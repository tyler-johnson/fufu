//! The patch body: one file's hunks, in git's unified diff.
//!
//! A patch format is not fufu's to spell. What this module contributes is
//! *correct content* — the same tree diff every stat surface already walks,
//! carried down to the line — and the shape it hands back is git's, so the
//! rendered result is something `git apply` takes and a person can paste
//! anywhere. Structured rather than raw bytes because both consumers need
//! the parts: the renderer colors by line kind, and `--json` carries kind
//! and text as fields.
//!
//! Hunk assembly is ours rather than [`gix::diff::blob::UnifiedDiff`]'s
//! because two details of git's format live in the boundaries, not in the
//! lines: a zero-length side numbers from the line *before* the insertion
//! point (`@@ -0,0 +1,3 @@`, not `-1,0`), and a file with no trailing
//! newline earns `\ No newline at end of file` on whichever version lacks
//! it. Both are what makes the output apply.

use std::ops::Range;

use serde::Serialize;

use crate::error::{Error, Result};

/// Symmetrical context lines around each change — git's default, and the
/// number `git apply` assumes when it looks for where a hunk goes.
const CONTEXT: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LineKind {
    Context,
    Insert,
    Delete,
}

/// One line of a hunk: what happened to it, and what it says.
///
/// `text` carries no line terminator and no `+`/`-`/space prefix — the
/// prefix is the `kind`, and a renderer that spelled it into the text would
/// force every other consumer to strip it back off. Content that is not
/// valid UTF-8 is carried lossily; git decides binary by NUL bytes, so a
/// latin-1 file reaches here as text and one that round-trips exactly is
/// not something a JSON string can promise anyway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PatchLine {
    pub kind: LineKind,
    pub text: String,
    /// This line ends its version of the file and that version has no
    /// trailing newline — git's `\ No newline at end of file`.
    pub no_newline: bool,
}

/// One hunk: where it lands in each version, and the lines between.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    /// The `@@ -a,b +c,d @@` line, exactly as it is printed.
    pub header: String,
    pub lines: Vec<PatchLine>,
}

/// The hunks between the two blobs a tree-diff change resolved to, or `None`
/// when either side is binary — the same signal `line_counts()` gives, so a
/// caller that already knows how to say "binary" has nothing new to learn.
pub(crate) fn hunks_of(
    platform: &mut gix::object::blob::diff::Platform<'_>,
) -> Result<Option<Vec<Hunk>>> {
    use gix::diff::blob::platform::prepare_diff::Operation;

    // gix's own `line_counts()` clears this before calling `prepare_diff()`
    // and then treats the external arm as `unreachable!`. Reaching
    // `prepare_diff()` directly means owing the same clearing: a repository
    // with `diff.external` configured would otherwise make this the one
    // place in the binary that spawns a process, and `tests/zero_spawn.rs`
    // would be the thing that noticed.
    platform
        .resource_cache
        .options
        .skip_internal_diff_if_external_is_configured = false;

    let prep = platform
        .resource_cache
        .prepare_diff()
        .map_err(Error::repo)?;
    let algorithm = match prep.operation {
        Operation::InternalDiff { algorithm } => algorithm,
        Operation::SourceOrDestinationIsBinary => return Ok(None),
        Operation::ExternalCommand { .. } => {
            unreachable!("the external differ is disabled directly above")
        }
    };

    // Terminators kept, unlike the interner `line_counts()` builds. Stripping
    // them makes a last line that gained a newline compare equal to one that
    // never had it — better-looking stats, and a patch that then silently
    // claims a trailing newline the file does not have.
    let input = gix::diff::blob::intern::InternedInput::new(
        prep.old.intern_source(),
        prep.new.intern_source(),
    );

    let mut changes: Vec<(Range<u32>, Range<u32>)> = Vec::new();
    gix::diff::blob::diff(
        algorithm,
        &input,
        |before: Range<u32>, after: Range<u32>| {
            changes.push((before, after));
        },
    );

    Ok(Some(assemble(&input, &changes)))
}

/// Group changes into hunks and fill in the lines between them.
///
/// Two changes share a hunk when the gap between them is no wider than the
/// context they would each print — otherwise the context would run together
/// and the reader could not tell one change from the next.
fn assemble(
    input: &gix::diff::blob::intern::InternedInput<&[u8]>,
    changes: &[(Range<u32>, Range<u32>)],
) -> Vec<Hunk> {
    let old_len = input.before.len() as u32;
    let line = |tokens: &[gix::diff::blob::intern::Token], at: u32, kind: LineKind| -> PatchLine {
        let raw: &[u8] = input.interner[tokens[at as usize]];
        let text = raw.strip_suffix(b"\n").unwrap_or(raw);
        PatchLine {
            kind,
            text: String::from_utf8_lossy(text).into_owned(),
            no_newline: text.len() == raw.len(),
        }
    };

    let mut hunks = Vec::new();
    let mut first = 0usize;
    while first < changes.len() {
        let mut last = first;
        while last + 1 < changes.len()
            && changes[last + 1]
                .0
                .start
                .saturating_sub(changes[last].0.end)
                <= 2 * CONTEXT
        {
            last += 1;
        }

        let head = &changes[first];
        let tail = &changes[last];
        let old_start = head.0.start.saturating_sub(CONTEXT);
        let old_end = (tail.0.end + CONTEXT).min(old_len);
        // Context regions run in lockstep, so the leading and trailing
        // context widths carry straight over to the new side.
        let new_start = head.1.start - (head.0.start - old_start);
        let new_end = tail.1.end + (old_end - tail.0.end);

        let mut lines = Vec::new();
        let mut pos = old_start;
        for change in &changes[first..=last] {
            for at in pos..change.0.start {
                lines.push(line(&input.before, at, LineKind::Context));
            }
            for at in change.0.clone() {
                lines.push(line(&input.before, at, LineKind::Delete));
            }
            for at in change.1.clone() {
                lines.push(line(&input.after, at, LineKind::Insert));
            }
            pos = change.0.end;
        }
        for at in pos..old_end {
            lines.push(line(&input.before, at, LineKind::Context));
        }

        let old_lines = old_end - old_start;
        let new_lines = new_end - new_start;
        hunks.push(Hunk {
            old_start,
            old_lines,
            new_start,
            new_lines,
            header: format!(
                "@@ -{} +{} @@",
                range(old_start, old_lines),
                range(new_start, new_lines)
            ),
            lines,
        });

        first = last + 1;
    }
    hunks
}

/// One side of a hunk header. Lines are 1-based, except that an empty side
/// numbers from the line *before* the insertion point — `-0,0` for a file
/// being created — and a single line drops the count entirely. Both are
/// git's spellings, and `git apply` reads them back.
fn range(start: u32, len: u32) -> String {
    let first = if len == 0 { start } else { start + 1 };
    if len == 1 {
        format!("{first}")
    } else {
        format!("{first},{len}")
    }
}
