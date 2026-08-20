//! One line of progress, for the one command that can run for minutes.
//!
//! Everything else fufu does finishes before a person could wonder whether it
//! had — so `gix::progress::Discard` is the right answer everywhere but here.
//! A clone pulls a pack over the wire, and silence for two minutes is
//! indistinguishable from a hang.
//!
//! What this is not is a progress tree. prodash ships renderers that draw one,
//! and they are a dependency (plus a render thread, plus a terminal state
//! machine) bought for a single line of output. So the nodes gix creates share
//! two counters instead — things counted, and bytes moved — and the line is
//! redrawn from those, throttled, on stderr.
//!
//! Silent unless stderr is a terminal: piped output and `--json` must stay
//! byte-identical to a run with no progress at all.

use std::fmt::Write as _;
use std::io::Write as _;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use gix::progress::{Id, MessageLevel, Step, StepShared, Unit};
use gix::{Count, NestedProgress, Progress};

/// How often the line is allowed to be redrawn. Fast enough to look live,
/// slow enough that a multi-threaded pack resolution is not writing to stderr
/// from four threads at once.
const THROTTLE_MS: u64 = 100;

/// State every node of one progress tree shares: whether to draw at all, the
/// phrase the line leads with, the throttle, and the counters to total.
struct Shared {
    on: bool,
    label: &'static str,
    start: Instant,
    /// Milliseconds since `start` at the last draw. Atomic rather than a
    /// mutex because pack resolution reports from several threads and a lock
    /// per increment would be the expensive part of a cheap operation.
    last_ms: AtomicU64,
    counters: Mutex<Vec<Tracked>>,
    /// High-water marks. gix reuses one counter across a phase's sub-steps
    /// and `set`s it back down when the next one starts — the remote's
    /// "counting objects" hands its counter straight to "compressing" — so
    /// the raw maximum falls, and a progress line that counts *down* reads
    /// as a bug rather than as progress. Each phase gets its own `Shared`,
    /// so a mark can never carry from receiving into checking out.
    peak_steps: AtomicUsize,
    peak_bytes: AtomicUsize,
}

/// One counter, and what it counts. Registered at `init`, which is where a
/// node declares its unit and stops being merely organizational.
struct Tracked {
    bytes: bool,
    counter: StepShared,
}

impl Shared {
    /// Register (or re-classify) a node's counter. `init` may be called more
    /// than once on the same node, so identity is the counter itself.
    fn track(&self, counter: &StepShared, bytes: bool) {
        let Ok(mut counters) = self.counters.lock() else {
            return;
        };
        match counters
            .iter_mut()
            .find(|t| Arc::ptr_eq(&t.counter, counter))
        {
            Some(existing) => existing.bytes = bytes,
            None => counters.push(Tracked {
                bytes,
                counter: counter.clone(),
            }),
        }
    }

    /// The two numbers the line shows: the largest count any counter has
    /// reached, and the largest byte total. The largest rather than the sum,
    /// because indexing and resolving both count the same objects — summing
    /// them would report twice as many objects as the pack holds — and a
    /// high-water mark rather than the current largest, so the line only ever
    /// moves forward.
    fn totals(&self) -> (Step, Step) {
        let (mut steps, mut bytes) = (0usize, 0usize);
        if let Ok(counters) = self.counters.lock() {
            for tracked in counters.iter() {
                let value = tracked.counter.load(Ordering::Relaxed);
                let slot = if tracked.bytes {
                    &mut bytes
                } else {
                    &mut steps
                };
                *slot = (*slot).max(value);
            }
        }
        (
            self.peak_steps
                .fetch_max(steps, Ordering::Relaxed)
                .max(steps),
            self.peak_bytes
                .fetch_max(bytes, Ordering::Relaxed)
                .max(bytes),
        )
    }

    /// Redraw, unless the throttle says not yet. The compare-exchange is what
    /// makes concurrent callers pick exactly one winner per window.
    fn draw(&self) {
        if !self.on {
            return;
        }
        let now = self.start.elapsed().as_millis() as u64;
        let last = self.last_ms.load(Ordering::Relaxed);
        if now.saturating_sub(last) < THROTTLE_MS {
            return;
        }
        if self
            .last_ms
            .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return;
        }
        let (steps, bytes) = self.totals();
        if steps == 0 && bytes == 0 {
            return;
        }
        let mut line = format!("\r{}: {}", self.label, thousands(steps));
        if bytes > 0 {
            let _ = write!(line, " ({})", size(bytes));
        }
        // Erase to end of line: the counts only grow, but a phase change can
        // shorten the line, and a stale tail would be a number nobody wrote.
        line.push_str("\x1b[K");
        let mut err = std::io::stderr();
        let _ = err.write_all(line.as_bytes());
        let _ = err.flush();
    }

    /// Take the line back off the terminal, so what a verb prints afterwards
    /// starts on a clean one.
    fn clear(&self) {
        if !self.on {
            return;
        }
        let mut err = std::io::stderr();
        let _ = err.write_all(b"\r\x1b[K");
        let _ = err.flush();
    }
}

/// One node of the progress tree. The root is [`Bar::new`]; gix makes the
/// rest with `add_child`.
pub struct Bar {
    shared: Arc<Shared>,
    counter: StepShared,
    name: String,
    id: Id,
    max: Option<Step>,
    unit: Option<Unit>,
}

impl Bar {
    /// A root node leading its line with `label`. Drawing is off unless
    /// `enabled` and stderr is a terminal — `--json` and piped output pass
    /// `false` and get a `Discard` in all but name.
    pub fn new(label: &'static str, enabled: bool) -> Self {
        let on = enabled && std::io::IsTerminal::is_terminal(&std::io::stderr());
        Bar {
            shared: Arc::new(Shared {
                on,
                label,
                start: Instant::now(),
                last_ms: AtomicU64::new(0),
                counters: Mutex::new(Vec::new()),
                peak_steps: AtomicUsize::new(0),
                peak_bytes: AtomicUsize::new(0),
            }),
            counter: Arc::new(AtomicUsize::new(0)),
            name: label.to_string(),
            id: gix::progress::UNKNOWN,
            max: None,
            unit: None,
        }
    }

    /// A handle that can take the line back off the terminal. gix consumes
    /// the bar itself, so the caller keeps one of these instead.
    pub fn handle(&self) -> Line {
        Line(self.shared.clone())
    }
}

/// The one thing a caller still needs after handing the bar to gix.
pub struct Line(Arc<Shared>);

impl Line {
    /// Erase the line, so what a verb prints next starts on a clean one.
    pub fn clear(&self) {
        self.0.clear();
    }
}

impl Count for Bar {
    fn set(&self, step: Step) {
        self.counter.store(step, Ordering::Relaxed);
        self.shared.draw();
    }

    fn step(&self) -> Step {
        self.counter.load(Ordering::Relaxed)
    }

    fn inc_by(&self, step: Step) {
        self.counter.fetch_add(step, Ordering::Relaxed);
        self.shared.draw();
    }

    fn counter(&self) -> StepShared {
        self.counter.clone()
    }
}

impl Progress for Bar {
    fn init(&mut self, max: Option<Step>, unit: Option<Unit>) {
        self.max = max;
        let bytes = unit.as_ref().is_some_and(is_bytes);
        self.unit = unit;
        self.shared.track(&self.counter, bytes);
    }

    fn unit(&self) -> Option<Unit> {
        self.unit.clone()
    }

    fn max(&self) -> Option<Step> {
        self.max
    }

    fn set_max(&mut self, max: Option<Step>) -> Option<Step> {
        std::mem::replace(&mut self.max, max)
    }

    fn set_name(&mut self, name: String) {
        self.name = name;
    }

    fn name(&self) -> Option<String> {
        Some(self.name.clone())
    }

    fn id(&self) -> Id {
        self.id
    }

    /// Dropped on purpose. This carries the remote's own chatter — "Counting
    /// objects", "Compressing" — and one line that moves says the same thing
    /// without interleaving somebody else's phrasing into fufu's output.
    fn message(&self, _level: MessageLevel, _message: String) {}
}

impl NestedProgress for Bar {
    type SubProgress = Bar;

    fn add_child(&mut self, name: impl Into<String>) -> Bar {
        self.add_child_with_id(name, gix::progress::UNKNOWN)
    }

    fn add_child_with_id(&mut self, name: impl Into<String>, id: Id) -> Bar {
        Bar {
            shared: self.shared.clone(),
            counter: Arc::new(AtomicUsize::new(0)),
            name: name.into(),
            id,
            max: None,
            unit: None,
        }
    }
}

/// Whether a unit counts bytes rather than things.
///
/// prodash keeps `Unit`'s kind private, so this asks by comparing labels —
/// against the label gix's own `bytes()` produces rather than a literal, because
/// which one that is depends on a gix feature (`""` from the size formatter,
/// `"B"` from the fallback). Counting units label themselves "objects",
/// "files", and so on, so the two never collide.
fn is_bytes(unit: &Unit) -> bool {
    static BYTES: OnceLock<String> = OnceLock::new();
    let bytes = BYTES.get_or_init(|| {
        gix::progress::bytes()
            .as_ref()
            .map(label)
            .unwrap_or_default()
    });
    label(unit) == *bytes
}

fn label(unit: &Unit) -> String {
    let mut out = String::new();
    let _ = unit.as_display_value().display_unit(&mut out, 1);
    out
}

/// `12431` → `12,431`. A pack's object count is six digits often enough that
/// the grouping is the difference between reading it and counting it.
fn thousands(value: Step) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (seen, ch) in digits.chars().rev().enumerate() {
        if seen > 0 && seen % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

/// Binary sizes, one decimal past a kibibyte. git says MiB here and so does
/// this: a person comparing the two numbers should not have to know which
/// base each of them chose.
fn size(bytes: Step) -> String {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = UNITS[0];
    for next in &UNITS[1..] {
        if value < 1024.0 {
            break;
        }
        value /= 1024.0;
        unit = next;
    }
    format!("{value:.1} {unit}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_grouped_in_threes() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(12_431), "12,431");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn sizes_are_binary_and_named_as_such() {
        assert_eq!(size(0), "0 B");
        assert_eq!(size(1_023), "1023 B");
        assert_eq!(size(1_024), "1.0 KiB");
        assert_eq!(size(4_404_019), "4.2 MiB");
        assert_eq!(size(3 * 1024 * 1024 * 1024), "3.0 GiB");
    }

    /// The classification the line depends on: gix's byte unit is bytes and
    /// its counting units are not, whichever way gix's features were set.
    #[test]
    fn the_byte_unit_is_told_apart_from_a_count() {
        assert!(is_bytes(
            &gix::progress::bytes().expect("gix has a byte unit")
        ));
        assert!(!is_bytes(
            &gix::progress::count("objects").expect("gix has a counting unit")
        ));
        assert!(!is_bytes(
            &gix::progress::count("files").expect("gix has a counting unit")
        ));
    }

    /// A child feeds the same two totals as its parent, and the largest of
    /// several counters wins rather than their sum.
    #[test]
    fn children_share_the_roots_counters() {
        let mut root = Bar::new("receiving objects", false);
        let mut indexing = root.add_child("indexing");
        indexing.init(None, gix::progress::count("objects"));
        let mut resolving = root.add_child("resolving");
        resolving.init(None, gix::progress::count("objects"));
        let mut pack = root.add_child("read pack");
        pack.init(None, gix::progress::bytes());

        indexing.inc_by(12_431);
        resolving.inc_by(9_000);
        pack.inc_by(4_404_019);

        assert_eq!(root.shared.totals(), (12_431, 4_404_019));
    }

    /// A counter gix resets between sub-phases must not make the line count
    /// down: the remote hands its "counting objects" counter to
    /// "compressing", which starts over from a smaller number.
    #[test]
    fn the_line_never_counts_backwards() {
        let mut root = Bar::new("receiving objects", false);
        let mut remote = root.add_child("remote");
        remote.init(None, gix::progress::count("objects"));

        remote.set(1_988);
        assert_eq!(root.shared.totals(), (1_988, 0));
        // The same counter, reused for the next sub-phase.
        remote.set(251);
        assert_eq!(root.shared.totals(), (1_988, 0));
        remote.set(2_500);
        assert_eq!(root.shared.totals(), (2_500, 0));
    }

    /// Nothing is drawn when drawing is off, and a node that never declared a
    /// unit contributes nothing at all.
    #[test]
    fn an_organizational_node_counts_toward_nothing() {
        let mut root = Bar::new("receiving objects", false);
        let headline = root.add_child("fetch");
        headline.inc_by(5);
        assert_eq!(root.shared.totals(), (0, 0));
    }
}
