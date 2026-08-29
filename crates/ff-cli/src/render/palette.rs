/// Dim is a modifier, not a color — it is never themed.
pub(super) const DIM: anstyle::Style = anstyle::Style::new().dimmed();

/// Bold is the other unthemed modifier, and it already means one thing in
/// this tool: *what you can type*. Op id columns bold their shortest unique
/// prefix and dim the rest for exactly that reason. A branch name is the
/// other typeable token on a row — it is what `ff switch` takes — so it wears
/// the same encoding rather than a tenth palette color. The current branch
/// adds the `at` green on top: bold says "you could go here", green says
/// "you are here".
pub(super) const BOLD: anstyle::Style = anstyle::Style::new().bold();

/// Nine semantic roles, each an ANSI style. Three themes are provided; the
/// process-global palette defaults to `MUTED` so every path works without
/// explicit initialization (tests, callers that forget, color-off pipes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub snap: anstyle::Style,
    pub sha: anstyle::Style,
    pub age: anstyle::Style,
    pub at: anstyle::Style,
    pub ins: anstyle::Style,
    pub del: anstyle::Style,
    pub ok: anstyle::Style,
    pub warn: anstyle::Style,
    pub ahead: anstyle::Style,
}

impl Palette {
    /// Desaturated 256-color — the default.
    pub const MUTED: Palette = Palette {
        snap: anstyle::Ansi256Color(139).on_default().bold(),
        sha: anstyle::Ansi256Color(67).on_default(),
        age: anstyle::Ansi256Color(73).on_default(),
        at: anstyle::Ansi256Color(71).on_default().bold(),
        ins: anstyle::Ansi256Color(71).on_default(),
        del: anstyle::Ansi256Color(167).on_default(),
        ok: anstyle::Ansi256Color(71).on_default(),
        warn: anstyle::Ansi256Color(173).on_default(),
        ahead: anstyle::Ansi256Color(67).on_default(),
    };

    /// Saturated 256-color — brighter, higher contrast.
    pub const VIVID: Palette = Palette {
        snap: anstyle::Ansi256Color(170).on_default().bold(),
        sha: anstyle::Ansi256Color(39).on_default(),
        age: anstyle::Ansi256Color(44).on_default(),
        at: anstyle::Ansi256Color(41).on_default().bold(),
        ins: anstyle::Ansi256Color(41).on_default(),
        del: anstyle::Ansi256Color(203).on_default(),
        ok: anstyle::Ansi256Color(41).on_default(),
        warn: anstyle::Ansi256Color(208).on_default(),
        ahead: anstyle::Ansi256Color(39).on_default(),
    };

    /// Base sixteen colors — lets the user's terminal theme decide the actual hues.
    pub const TERMINAL: Palette = Palette {
        snap: anstyle::AnsiColor::Magenta.on_default().bold(),
        sha: anstyle::AnsiColor::Blue.on_default(),
        age: anstyle::AnsiColor::Cyan.on_default(),
        at: anstyle::AnsiColor::Green.on_default().bold(),
        ins: anstyle::AnsiColor::Green.on_default(),
        del: anstyle::AnsiColor::Red.on_default(),
        ok: anstyle::AnsiColor::Green.on_default(),
        warn: anstyle::AnsiColor::Yellow.on_default(),
        ahead: anstyle::AnsiColor::Blue.on_default(),
    };
}

static PALETTE: std::sync::OnceLock<Palette> = std::sync::OnceLock::new();

/// Store the palette for the process. First call wins; subsequent calls are
/// silently ignored so a caller that initializes twice does not panic.
pub fn set_palette(p: Palette) {
    // OnceLock::set returns Err if already initialized — we drop it because
    // the first winner stands and a double-init is harmless.
    let _ = PALETTE.set(p);
}

/// The current palette, or `MUTED` when nothing was set.
pub fn palette() -> &'static Palette {
    PALETTE.get().unwrap_or(&Palette::MUTED)
}

/// Map a config string to a palette. Unrecognized values and `None` yield `MUTED`.
pub fn palette_for(name: Option<&str>) -> Palette {
    match name {
        Some(n) => match n.to_lowercase().as_str() {
            "vivid" => Palette::VIVID,
            "terminal" => Palette::TERMINAL,
            _ => Palette::MUTED,
        },
        None => Palette::MUTED,
    }
}

/// Read `fufu.theme` from the repo config and install the matching palette.
pub fn init_palette(repo: &ff_core::gix::Repository) {
    let theme = repo
        .config_snapshot()
        .string("fufu.theme")
        .map(|s| s.to_string());
    set_palette(palette_for(theme.as_deref()));
}

/// Paint `text`, or hand it back untouched when color is off or it's empty.
pub(super) fn paint(text: &str, style: anstyle::Style, colored: bool) -> String {
    if !colored || text.is_empty() {
        return text.to_string();
    }
    format!("{}{text}{}", style.render(), style.render_reset())
}

/// A left-aligned column: pad FIRST (format-width would count escape bytes).
pub fn col(text: &str, width: usize, style: anstyle::Style, colored: bool) -> String {
    let pad = " ".repeat(width.saturating_sub(text.chars().count()));
    format!("{}{pad}", paint(text, style, colored))
}

/// A right-aligned column, same escape-safe padding.
pub(super) fn col_right(text: &str, width: usize, style: anstyle::Style, colored: bool) -> String {
    let pad = " ".repeat(width.saturating_sub(text.chars().count()));
    format!("{pad}{}", paint(text, style, colored))
}

/// A snapshot id column: pad FIRST (format-width would count escape bytes),
/// then brighten the shortest-unique prefix and dim the rest — "the bold
/// part is what you can type". Snapshot ids only; commit shas are plain.
pub fn styled_id(display: &str, unique: usize, width: usize, colored: bool) -> String {
    let pad = " ".repeat(width.saturating_sub(display.chars().count()));
    if !colored {
        return format!("{display}{pad}");
    }
    let (head, tail) = display.split_at(unique.min(display.len()));
    format!(
        "{}{}{pad}",
        paint(head, palette().snap, colored),
        paint(tail, DIM, colored)
    )
}

/// Painter helpers — one-liners that command files use instead of touching
/// `anstyle` directly. Each takes `(text, colored)` and delegates to the
/// private `paint` with the correct semantic role from the current palette.
pub fn paint_sha(text: &str, colored: bool) -> String {
    paint(text, palette().sha, colored)
}

/// An operation id, whole. The log family's id role — the same magenta the
/// highlighted prefix wears, spent where there is no prefix to highlight.
pub fn paint_id(text: &str, colored: bool) -> String {
    paint(text, palette().snap, colored)
}

pub fn paint_ok(text: &str, colored: bool) -> String {
    paint(text, palette().ok, colored)
}

pub fn paint_warn(text: &str, colored: bool) -> String {
    paint(text, palette().warn, colored)
}

pub fn paint_ahead(text: &str, colored: bool) -> String {
    paint(text, palette().ahead, colored)
}

pub fn paint_dim(text: &str, colored: bool) -> String {
    paint(text, DIM, colored)
}

#[cfg(test)]
mod tests {
    use super::{Palette, palette_for, styled_id};

    #[test]
    fn styled_id_pads_before_ansi() {
        // Plain: padding only.
        assert_eq!(styled_id("abc", 2, 5, false), "abc  ");
        // Colored: trailing pad spaces sit outside the escapes, and the
        // visible text is intact.
        let styled = styled_id("abc", 2, 5, true);
        assert!(styled.ends_with("  "), "pad after reset: {styled:?}");
        assert!(styled.contains("ab"), "prefix present");
        assert!(styled.contains('c'), "tail present");
        // Width shorter than the id: no pad, no truncation.
        assert_eq!(styled_id("abcdef", 3, 4, false), "abcdef");
    }

    #[test]
    fn palette_defaults_to_muted() {
        assert_eq!(palette_for(None), Palette::MUTED);
        assert_eq!(palette_for(Some("nonsense")), Palette::MUTED);
    }

    #[test]
    fn palette_parses_case_insensitively() {
        assert_eq!(palette_for(Some("VIVID")), Palette::VIVID);
        assert_eq!(palette_for(Some("Terminal")), Palette::TERMINAL);
        assert_eq!(palette_for(Some("muted")), Palette::MUTED);
    }

    // The OnceLock behind `palette()` is process-global, so a test that sets
    // it races every other test in the same binary — cargo runs them as
    // threads. `palette_for` is where the real mapping logic lives and it is
    // pure; the set-once plumbing is std behavior and is covered end-to-end by
    // the `ff config theme` integration tests instead.
}
