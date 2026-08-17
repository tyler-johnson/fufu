//! The commit-graph gutter: lanes, rails, and connectors drawn in front of
//! each row's payload, without knowing what a row is. It is kept out of
//! `render.rs` so `ff log --graph` can hang its own rows off the same lane
//! machinery later.

use crate::render::paint_dim;

/// One row of the graph: where its parents are, what glyph sits in its lane,
/// and the payload that rides beside it.
#[derive(Debug, Clone)]
pub struct GraphRow<'a> {
    /// Row indices of this row's parents. Every index is greater than this
    /// row's own, so the graph reads top-down as newest-to-oldest.
    pub parents: &'a [usize],
    /// The node glyph, already painted by the caller. Exactly one display
    /// column wide; the renderer treats it as opaque.
    pub glyph: &'a str,
    /// One or two lines. Line 0 rides the node line; line 1, when present,
    /// rides the edge line where the lane transitions are drawn.
    pub payload: &'a [String],
}

/// Draw the gutter in front of every row's payload. `colored` decides
/// whether the rails and connectors are dimmed.
pub fn render(rows: &[GraphRow<'_>], colored: bool) -> Vec<String> {
    // Each lane is the row it is waiting for, or free. A parent already
    // riding a lane is joined, never duplicated, so one row can never sit in
    // two lanes and the lanes can never have to merge back together.
    let mut lanes: Vec<Option<usize>> = Vec::new();
    let mut out: Vec<String> = Vec::new();

    for (i, row) in rows.iter().enumerate() {
        // Claim a lane: the one a parent already reserved for this row, else
        // the first free lane, else a fresh one.
        let col = match lanes.iter().position(|lane| *lane == Some(i)) {
            Some(col) => col,
            None => first_free_lane(&mut lanes),
        };
        lanes[col] = Some(i);

        // Node line: the glyph in its lane, a rail where another row is
        // still waiting, a space elsewhere.
        let width = lanes
            .iter()
            .rposition(|lane| lane.is_some())
            .map(|lane| lane + 1)
            .unwrap_or(0)
            .max(col + 1);
        let mut line = (0..width)
            .map(|l| node_cell(l, col, &lanes, row.glyph, colored))
            .collect::<Vec<_>>()
            .join(" ");
        line.push_str("  ");
        line.push_str(&row.payload[0]);
        out.push(line.trim_end().to_string());

        // Transition: this row's lane is released, then the parents are
        // pointed at in parent order.
        lanes[col] = None;
        let mut dests: Vec<usize> = Vec::new();
        let mut pre_occupied: Vec<bool> = Vec::new();
        // Lanes a leftward join emptied: they draw a closing `╯` on this
        // line and are free from the next row on.
        let mut closed: Vec<usize> = Vec::new();
        for &parent in row.parents {
            match lanes.iter().position(|lane| *lane == Some(parent)) {
                // A join: the parent already owns a lane.
                Some(l) => {
                    // Lanes collapse leftward, never right. Which chain
                    // reached a shared parent first is an accident of
                    // emission order, and letting it keep the lane pushes
                    // trunk rightward while the lane beside it dies — the
                    // rail then curves out to meet it (`╰─┤`) instead of the
                    // branch folding back in (`├─╯`). Moving the wait to this
                    // row's own lane, when it is free, keeps the spine on the
                    // left where a reader looks for it. The one-lane-per-row
                    // invariant survives: the wait moves, it is not copied.
                    let d = if l > col && lanes[col].is_none() {
                        lanes[l] = None;
                        // The vacated lane still has a rail coming down into
                        // this line and must be closed off, not just dropped.
                        closed.push(l);
                        lanes[col] = Some(parent);
                        col
                    } else {
                        l
                    };
                    dests.push(d);
                    // The lane it arrived on is the one that was already
                    // riding; after a move, this row's lane is fresh below.
                    pre_occupied.push(d != col);
                }
                None => {
                    // This row's lane goes first while it is still free; the
                    // first free lane is only the fallback.
                    let d = if lanes[col].is_none() {
                        col
                    } else {
                        first_free_lane(&mut lanes)
                    };
                    lanes[d] = Some(parent);
                    dests.push(d);
                    pre_occupied.push(false);
                }
            }
        }

        // Edge line: what the transition itself draws.
        let down = dests.contains(&col);
        let left = dests.iter().any(|&d| d < col);
        // A closed lane is always to the right of `col`, and the fold arrives
        // at `col` from that side — so it is a right-going connection here,
        // and a left-going one (`╯`) at the lane it vacated.
        let right = dests.iter().any(|&d| d > col) || !closed.is_empty();

        let mut hi = col;
        for &d in dests.iter().chain(&closed) {
            hi = hi.max(d);
        }
        if let Some(last) = lanes.iter().rposition(|lane| lane.is_some()) {
            hi = hi.max(last);
        }

        let mut line = String::new();
        for l in 0..=hi {
            if l > 0 {
                // A lane occupied by some other row keeps its rail even
                // inside the span: the vertical wins and the horizontal reads
                // as crossing behind it. Deliberate, not an oversight.
                if crosses(l, col, &dests) || crosses(l, col, &closed) {
                    line.push_str(&paint_dim("─", colored));
                } else {
                    line.push(' ');
                }
            }
            line.push_str(&edge_cell(
                l,
                &lanes,
                &Transition {
                    col,
                    dests: &dests,
                    closed: &closed,
                    pre_occupied: &pre_occupied,
                    down,
                    left,
                    right,
                },
                colored,
            ));
        }
        if let Some(second) = row.payload.get(1) {
            line.push_str("  ");
            line.push_str(second);
        }
        let line = line.trim_end().to_string();

        // The edge line is dropped only when the lanes do not actually move
        // and no second payload line is left to hang on a rail.
        // A closed lane is movement even when every dest landed on `col`:
        // the fold has to be drawn or the rail above it just stops.
        let trivial =
            closed.is_empty() && (dests.is_empty() || (dests.len() == 1 && dests[0] == col));
        if !trivial || row.payload.len() > 1 {
            out.push(line);
        }
    }

    out
}

/// The first free lane, or a freshly pushed one when every lane is waiting.
fn first_free_lane(lanes: &mut Vec<Option<usize>>) -> usize {
    match lanes.iter().position(|lane| lane.is_none()) {
        Some(l) => l,
        None => {
            lanes.push(None);
            lanes.len() - 1
        }
    }
}

/// The node-line cell at lane `l`. The glyph rides through unpainted: the
/// caller already painted it, and painting it again would double-wrap.
fn node_cell(l: usize, col: usize, lanes: &[Option<usize>], glyph: &str, colored: bool) -> String {
    if l == col {
        glyph.to_string()
    } else if lanes[l].is_some() {
        paint_dim("│", colored)
    } else {
        " ".to_string()
    }
}

/// What one row's transition does to the lanes: which lane the row sat in,
/// where its flow went, and which of those lanes the flow was already riding.
/// Bundled because the edge line answers all of it at every cell.
struct Transition<'a> {
    col: usize,
    dests: &'a [usize],
    /// Lanes a leftward join emptied on this line: they draw a closing `╯`.
    closed: &'a [usize],
    pre_occupied: &'a [bool],
    down: bool,
    left: bool,
    right: bool,
}

/// Whether the gap between lanes `l - 1` and `l` lies inside the span some
/// parent reached from `col`: the transition's horizontal crosses it.
fn crosses(l: usize, col: usize, dests: &[usize]) -> bool {
    dests.iter().any(|&d| d.min(col) < l && l <= d.max(col))
}

/// The edge-line cell at lane `l`.
fn edge_cell(l: usize, lanes: &[Option<usize>], t: &Transition<'_>, colored: bool) -> String {
    let Transition {
        col,
        dests,
        closed,
        pre_occupied,
        down,
        left,
        right,
    } = *t;
    let ch = if l == col {
        // This row's lane: the up side is the node itself, so the cell only
        // answers to what the transition adds below and to the sides.
        match (down, left, right) {
            (true, true, true) => Some('┼'),
            (true, false, true) => Some('├'),
            (true, true, false) => Some('┤'),
            (true, false, false) => Some('│'),
            (false, true, true) => Some('┴'),
            (false, false, true) => Some('╰'),
            (false, true, false) => Some('╯'),
            (false, false, false) => None,
        }
    } else if closed.contains(&l) {
        // Up from the rail above, left into the lane it folded into.
        Some('\u{256f}')
    } else if let Some(d) = dests.iter().position(|&x| x == l) {
        // A lane the transition pointed at: the down side is new, and the up
        // side exists only if the parent was already riding it.
        if l < col {
            Some(if pre_occupied[d] { '├' } else { '╭' })
        } else {
            Some(if pre_occupied[d] { '┤' } else { '╮' })
        }
    } else if lanes[l].is_some() {
        Some('│')
    } else {
        None
    };
    match ch {
        Some(c) => paint_dim(&c.to_string(), colored),
        None => " ".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{GraphRow, render};

    /// One test row. The payload `Vec<String>`s must stay alive in the
    /// caller's locals — the row only borrows them.
    fn row<'a>(parents: &'a [usize], glyph: &'a str, payload: &'a [String]) -> GraphRow<'a> {
        GraphRow {
            parents,
            glyph,
            payload,
        }
    }

    fn expect(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn linear_chain() {
        let p0 = vec!["head".into(), "subject one".into()];
        let p1 = vec!["second".into(), "subject two".into()];
        let p2 = vec!["".into()];
        let rows = [row(&[1], "@", &p0), row(&[2], "●", &p1), row(&[], "~", &p2)];
        assert_eq!(
            render(&rows, false),
            expect(&[
                "@  head",
                "│  subject one",
                "●  second",
                "│  subject two",
                "~",
            ])
        );
    }

    #[test]
    fn a_fork_and_a_join() {
        let p0 = vec!["open".into(), "pending".into()];
        let p1 = vec!["tip".into(), "tip subject".into()];
        let p2 = vec!["4 commits".into()];
        let p3 = vec!["other".into(), "other subject".into()];
        let p4 = vec!["trunk".into(), "trunk subject".into()];
        let p5 = vec!["".into()];
        let rows = [
            row(&[1], "@", &p0),
            row(&[2], "●", &p1),
            row(&[4], "~", &p2),
            row(&[4], "●", &p3),
            row(&[5], "●", &p4),
            row(&[], "~", &p5),
        ];
        assert_eq!(
            render(&rows, false),
            expect(&[
                "@  open",
                "│  pending",
                "●  tip",
                "│  tip subject",
                "~  4 commits",
                "│ ●  other",
                "├─╯  other subject",
                "●  trunk",
                "│  trunk subject",
                "~",
            ])
        );
    }

    #[test]
    fn a_merge_opens_a_lane() {
        let p0 = vec!["merge".into(), "merged feature".into()];
        let p1 = vec!["main side".into(), "m1".into()];
        let p2 = vec!["feature side".into(), "x1".into()];
        let p3 = vec!["base".into(), "b".into()];
        let rows = [
            row(&[1, 2], "●", &p0),
            row(&[3], "●", &p1),
            row(&[3], "●", &p2),
            row(&[], "●", &p3),
        ];
        assert_eq!(
            render(&rows, false),
            expect(&[
                "●  merge",
                "├─╮  merged feature",
                "● │  main side",
                "│ │  m1",
                "│ ●  feature side",
                "├─╯  x1",
                "●  base",
                "   b",
            ])
        );
    }

    #[test]
    fn a_one_line_row_with_a_moving_lane_gets_an_edge_line() {
        let p0 = vec!["a".into(), "as".into()];
        let p1 = vec!["3 commits".into()];
        let p2 = vec!["b".into(), "bs".into()];
        let rows = [row(&[2], "●", &p0), row(&[2], "~", &p1), row(&[], "●", &p2)];
        assert_eq!(
            render(&rows, false),
            expect(&["●  a", "│  as", "│ ~  3 commits", "├─╯", "●  b", "   bs",])
        );
    }

    #[test]
    fn no_parents_no_edge_line() {
        let p0 = vec!["only".into()];
        let rows = [row(&[], "@", &p0)];
        assert_eq!(render(&rows, false), expect(&["@  only"]));
    }

    /// The shared parent is claimed by the *right-hand* branch first, so a
    /// naive "reuse whatever lane already waits" rule would push trunk out to
    /// lane 1 and curve the elision rightward to meet it. Trunk belongs on the
    /// left, so the wait moves and lane 1 folds back in.
    #[test]
    fn a_join_collapses_leftward_even_when_the_right_lane_claimed_it_first() {
        let p0 = vec!["a".into(), "as".into()];
        let p1 = vec!["b".into(), "bs".into()];
        let p2 = vec!["2 commits".into()];
        let p3 = vec!["main".into(), "ms".into()];
        let rows = [
            row(&[2], "●", &p0),
            row(&[3], "●", &p1),
            row(&[3], "~", &p2),
            row(&[], "●", &p3),
        ];
        assert_eq!(
            render(&rows, false),
            expect(&[
                "●  a",
                "│  as",
                "│ ●  b",
                "│ │  bs",
                "~ │  2 commits",
                "├─╯",
                "●  main",
                "   ms",
            ])
        );
    }

    #[test]
    fn colored_wraps_only_the_connectors() {
        let p0 = vec!["head".into(), "subject one".into()];
        let p1 = vec!["second".into(), "subject two".into()];
        let p2 = vec!["".into()];
        let rows = [row(&[1], "@", &p0), row(&[2], "●", &p1), row(&[], "~", &p2)];
        let out = render(&rows, true);
        let joined = out.join("\n");
        assert!(joined.contains("head"), "payload must come through intact");
        assert!(
            joined.contains("subject one"),
            "payload must come through intact"
        );
        // At least one connector is dimmed, i.e. wrapped in ANSI escapes.
        // Do not assert the exact bytes: the palette is process-global and
        // another test in this binary could set it.
        assert!(out.iter().any(|line| line.contains('\u{1b}')));
        // The caller's glyphs ride through unpainted, in their lanes.
        assert!(out[0].starts_with('@'));
        assert!(out[2].starts_with('●'));
        assert!(out[4].starts_with('~'));
    }
}
