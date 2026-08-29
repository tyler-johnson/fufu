//! The marker shapes. A **fufu block** is an opener line, any number of lines,
//! then a closer line; the ours label is fixed and the closer carries the step
//! that wrote it. Anything that does not match these exact shapes is foreign
//! content — a file that legitimately contains conflict markers (a test
//! fixture, a document about merging) must not be mistaken for fufu's own.

/// The stem of the label on the *ours* side of a chain's conflict markers:
/// the replay as it stands, everything before this step already folded in.
/// Each step appends its own `(k/n)`, exactly as the theirs side does, so a
/// file collecting more than one block wears no two identical marker lines —
/// a reader knows which step they are looking at without scrolling to the
/// closer, and nothing downstream has to tell two anchors apart by position.
pub(super) const CHAIN_OURS: &str = "the rewrite so far";

/// The stem every fufu opener starts with; each carries its own `(k/n)`
/// after it, so the whole line is matched by prefix.
pub(super) const OPENER: &str = "<<<<<<< the rewrite so far";
const CLOSER_PREFIX: &str = ">>>>>>> rebasing \"";

/// A line that opens a fufu conflict block.
fn is_opener(line: &str) -> bool {
    line.trim_end_matches('\n').starts_with(OPENER)
}

/// A line that separates the ours and theirs halves of a conflict block.
fn is_separator(line: &str) -> bool {
    line.trim_end_matches('\n') == "======="
}

/// The 0-based step a closer line encodes, `None` when it is not a well-formed
/// fufu closer: the tail after the subject's closing quote must read
/// ` (<k>/<n>)` with both decimal and `k` at least one.
fn closer_step(line: &str) -> Option<usize> {
    let l = line.trim_end_matches('\n');
    let after = l.strip_prefix(CLOSER_PREFIX)?;
    // The subject sits between the first and the last quote; the step tail
    // follows the last one. A subject containing a quote is therefore taken
    // by its outermost quotes, not by escaping.
    let q = after.rfind('"')?;
    let tail = after[q..].strip_prefix('"')?;
    let tail = tail.strip_prefix(' ')?;
    let tail = tail.strip_prefix('(')?;
    let tail = tail.strip_suffix(')')?;
    let (k, _n) = tail.split_once('/')?;
    let k = k.parse::<usize>().ok()?;
    k.checked_sub(1)
}

/// A line that closes a fufu conflict block, with a parseable step.
fn is_closer(line: &str) -> bool {
    closer_step(line).is_some()
}

/// One fufu conflict block, located.
#[derive(Debug, Clone)]
pub(super) struct Block {
    pub(super) step: usize,
    pub(super) text: String,
    /// The block's half-open line range in the text it was found in.
    pub(super) lines: std::ops::Range<usize>,
}

/// Every fufu conflict block in `text`, in the order they appear, and whether
/// anything in the text is tangled. A tangle is a block the parser cannot
/// trust: an opener with no closer after it, a nested opener inside a block,
/// or more than one separator line inside a block. It is a value, not a
/// failure, so both are returned.
pub(super) fn blocks(text: &str) -> (Vec<Block>, bool) {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut found: Vec<Block> = Vec::new();
    let mut tangled = false;

    let mut i = 0usize;
    while i < lines.len() {
        if !is_opener(lines[i]) {
            i += 1;
            continue;
        }
        // Scan forward to the first closer, or a nested opener, whichever
        // comes first.
        let mut j = i + 1;
        while j < lines.len() && !is_opener(lines[j]) && !is_closer(lines[j]) {
            j += 1;
        }
        if j < lines.len() && is_opener(lines[j]) {
            // A nested opener before any closer: tangle; resume past it.
            tangled = true;
            i = j + 1;
            continue;
        }
        if j < lines.len() && is_closer(lines[j]) {
            // More than one separator between the opener and the closer means
            // the region interleaved rather than nested.
            let separators = (i + 1..j).filter(|&m| is_separator(lines[m])).count();
            if separators > 1 {
                tangled = true;
            } else if let Some(step) = closer_step(lines[j]) {
                found.push(Block {
                    step,
                    text: lines[i..=j].concat(),
                    lines: i..j + 1,
                });
            }
            i = j + 1;
            continue;
        }
        // An opener with no closer at all: tangle.
        tangled = true;
        i += 1;
    }

    (found, tangled)
}
