//! `ff history` — the moves, not the operations.
//!
//! `ff op log` answers *what happened*. This answers the other question, the
//! one a person actually has at the moment they open a log: **where can I go
//! back to?** That is `ff undo`'s granularity and not the log's — DESIGN
//! already draws the line ("Undo moves by runs, not by operations. A capture
//! is a machine's granularity and a person's undo is not"), and until now
//! nothing rendered it.
//!
//! So one row is one keystroke. A run of captures is one row because it is one
//! `ff undo`; the redo path sits above `@` because those are keystrokes too,
//! in the other direction. The two halves come from the two functions `ff
//! undo` and `ff redo` already use — `walk::run_at` stepping by `run.prev`,
//! and the reflog stack behind `forward_target` — rather than from a second
//! reading of the same rules, which is how a view and the verb it describes
//! come apart.
//!
//! Linear in operations, because `run_at` walks a run to count what it
//! collapsed: the same cost `ff undo` already pays every time it is typed.
//! The `fufu-prev-verb` link would hop each boundary in O(1) at the price of
//! that count, and the count is worth more until a profile says otherwise.

use serde::Serialize;

use crate::error::Result;
use crate::ops::{OpId, OpLog};

/// How a row is reached from where the repository stands now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Move {
    /// Where the log stands. Exactly one row is this one.
    Now,
    /// Reached by stepping back — `distance` presses of `ff undo`.
    Undo,
    /// Reached by stepping forward — `-distance` presses of `ff redo`.
    Redo,
}

impl Move {
    pub fn as_str(self) -> &'static str {
        match self {
            Move::Now => "now",
            Move::Undo => "undo",
            Move::Redo => "redo",
        }
    }
}

/// One row: one operation you can stand on, and how many keystrokes away.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    /// The operation, spelled in letters like every other op address.
    pub id: String,
    /// The shortest prefix the `ff op` verbs resolve unambiguously.
    pub short_id: String,
    /// Which keystroke reaches it.
    pub landing: Move,
    /// The operation's own kind — `op`, `capture`, `foreign`, `note`.
    pub kind: String,
    pub summary: String,
    pub time: i64,
    pub branch: Option<String>,
    pub session: Option<String>,
    /// How many operations the step onto this row traversed. A keystroke that
    /// moved forty operations must not have to be inferred, so it is
    /// reported. `1` is the ordinary answer and `0` is `@`, which is not
    /// stepped onto at all.
    pub collapsed: usize,
    /// Signed keystrokes from now: `0` is now, positive is presses of `ff
    /// undo`, negative is presses of `ff redo`.
    pub distance: i64,
}

/// The rows, redo path first (furthest forward at the top), then `@`, then
/// the undo path.
///
/// `limit` bounds the undo path only — the direction that grows without
/// bound. `0` means unlimited. The redo path is a reflog stack and is short
/// by construction, and truncating it would leave a row's `distance` claiming
/// a number of keystrokes the list does not show.
///
/// Whether the oldest row is the floor is the caller's to read off the list
/// rather than a field on a row: fewer undo rows than `limit` means the walk
/// ran out, and that is a fact about the listing, not about the operation
/// that happens to sit at its end.
pub fn history(repo: &gix::Repository, limit: usize) -> Result<Vec<Step>> {
    let log = OpLog::open(repo)?;
    let Some(tip) = log.tip()? else {
        return Ok(Vec::new());
    };

    let mut rows: Vec<Step> = Vec::new();

    // Forward first, so the vector reads top to bottom the way the screen
    // does: the furthest redo at the top, `@` in the middle.
    let forward = crate::undo::forward_targets(repo, tip)?;
    for (idx, id) in forward.iter().enumerate().rev() {
        rows.push(step(&log, *id, Move::Redo, -(idx as i64 + 1), 1)?);
    }

    rows.push(step(&log, tip, Move::Now, 0, 0)?);

    let mut cursor = tip;
    let mut distance = 0i64;
    loop {
        if limit != 0 && distance as usize >= limit {
            break;
        }
        // Notes are stepped over rather than landed on, exactly as `ff undo`
        // steps over them: a note marks something that happened rather than
        // something that was done, and there is no state behind it to put
        // back. Running out of them is the floor.
        let Some(run) = crate::undo::undoable_run_at(repo, cursor)? else {
            break;
        };
        let Some(prev) = run.prev else { break };
        distance += 1;
        rows.push(step(&log, prev, Move::Undo, distance, run.len)?);
        cursor = prev;
    }

    // Abbreviation is priced by the rows on screen, the same rule `ff op log`
    // follows: one index pass over exactly what is being shown.
    let hexes: Vec<String> = rows
        .iter()
        .map(|row| OpId::parse(&row.id).map(|id| id.hex()))
        .collect::<Result<_>>()?;
    let lens = crate::ops::index::prefix_lens(repo, &hexes)?;
    for (row, hex) in rows.iter_mut().zip(&hexes) {
        let len = lens.get(hex).copied().unwrap_or(8).max(4);
        row.short_id = row.id.chars().take(len).collect();
    }

    Ok(rows)
}

fn step(log: &OpLog<'_>, id: OpId, landing: Move, distance: i64, collapsed: usize) -> Result<Step> {
    let op = log.get(id)?;
    Ok(Step {
        id: op.id().to_string(),
        // Filled by the caller, once the whole list is known.
        short_id: String::new(),
        landing,
        kind: op.kind().as_str().to_string(),
        summary: op.summary().to_string(),
        time: op.time(),
        branch: op.branch().map(str::to_string),
        session: op.session().map(str::to_string),
        collapsed,
        distance,
    })
}

/// The run walk is the same one `ff undo` uses, so this module has no
/// stepping rule of its own to test. What it does have is the agreement
/// between the two, and that is pinned end to end in `ff-cli`'s tests where
/// both verbs can actually be run.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_move_names_are_the_ones_the_rows_print() {
        assert_eq!(Move::Now.as_str(), "now");
        assert_eq!(Move::Undo.as_str(), "undo");
        assert_eq!(Move::Redo.as_str(), "redo");
    }
}
