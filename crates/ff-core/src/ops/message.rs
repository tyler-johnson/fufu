//! The op commit message: a subject, an optional skipped-files block, and
//! the trailer paragraph that is the decoder's schema.
//!
//! The skeleton lives in trailers rather than in `op.json` for one reason,
//! and it is arithmetic. `segment_anchors` and the evolog read base, prev and
//! the segment link once per displayed row; reaching them through the record
//! would turn one object read per row into four (op commit, record commit,
//! record tree, `op.json` blob) on a path that is CI-gated flat. `op.json`
//! still carries the same fields and stays authoritative for the machine
//! surface — [`super::record::OpRecord`] is written from the same variables
//! these trailers are, and a test pins the agreement.
//!
//! Every key is written on every op, `none` included, so a *missing* key
//! means "not written by this fufu" rather than "absent value" — the
//! distinction a decoder needs when a chain predates a field.

use crate::ops::OpKind;

/// Subjects are capped so `log --oneline`-style rendering stays sane.
pub const MAX_SUBJECT: usize = 120;

/// Which kind of operation this is. Read before anything else: it is what
/// tells the decoder whether a record commit exists to fetch at all, and a
/// capture — the overwhelming majority of every log — has none.
const KIND_KEY: &str = "fufu-kind";
/// The chain the op ran on: a branch name, or `@detached`.
const BRANCH_KEY: &str = "fufu-branch";
/// HEAD's commit when the op ran — the base edge, at a fixed parent slot.
const BASE_KEY: &str = "fufu-base";
/// The previous operation anywhere in the log: the first-parent chain,
/// stated rather than inferred. Parent slot 1 holds it only when one exists,
/// and "is parent 1 parentless" costs an object read per row to ask.
const PREV_KEY: &str = "fufu-prev";
/// The previous operation on this op's own branch.
const PREV_BRANCH_KEY: &str = "fufu-prev-branch";
/// The newest op of the segment before this one (see `evolog`'s
/// `segment_anchors`): the hop that skips a run of same-base ops whole.
const PREV_SEGMENT_KEY: &str = "fufu-prev-segment";
/// The blob holding the last-seen ref table. A verb op writes a fresh one
/// (its plan); a capture copies its predecessor's oid verbatim.
const REFS_KEY: &str = "fufu-refs";
/// The session tag, when one was set.
const SESSION_KEY: &str = "fufu-session";

const NONE: &str = "none";

/// What an op knows about the segment before its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentLink {
    /// This op's segment is the branch's first: there is nothing earlier.
    ChainStart,
    /// The newest op of the previous segment.
    At(gix::ObjectId),
}

/// Everything the decoder reads without touching a second object. Written
/// from one set of variables, so [`build`] and [`parse`] are inverses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skeleton {
    pub kind: OpKind,
    pub branch: Option<String>,
    pub base: Option<gix::ObjectId>,
    pub prev: Option<gix::ObjectId>,
    pub prev_on_branch: Option<gix::ObjectId>,
    pub prev_segment: Option<SegmentLink>,
    pub refs_blob: Option<gix::ObjectId>,
    pub session: Option<String>,
}

impl Skeleton {
    pub fn new(kind: OpKind) -> Self {
        Skeleton {
            kind,
            branch: None,
            base: None,
            prev: None,
            prev_on_branch: None,
            prev_segment: None,
            refs_blob: None,
            session: None,
        }
    }
}

/// Collapse whitespace runs to single spaces and cap at `max` characters,
/// appending `…` when truncated.
pub fn clean_subject(raw: &str, max: usize) -> String {
    let mut out = String::with_capacity(raw.len().min(max));
    let mut last_space = true; // leading whitespace is dropped
    for ch in raw.chars() {
        if ch.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    while out.ends_with(' ') {
        out.pop();
    }
    if out.chars().count() > max {
        out = out.chars().take(max.saturating_sub(1)).collect();
        while out.ends_with(' ') {
            out.pop();
        }
        out.push('…');
    }
    out
}

/// The full message: subject, the skipped block when capture dropped
/// oversize files, then the trailer paragraph. The skipped list stays in the
/// body rather than moving into the record, because the ops that have one
/// are precisely the captures, which have no record.
pub fn build(subject: &str, skipped: &[String], skeleton: &Skeleton) -> String {
    let mut msg = clean_subject(subject, MAX_SUBJECT);
    if !skipped.is_empty() {
        msg.push_str("\n\nSkipped (fufu.maxFileSize):\n");
        for path in skipped {
            msg.push_str(path);
            msg.push('\n');
        }
        msg.pop(); // the trailer paragraph re-adds the separator
    }
    msg.push_str("\n\n");
    trailer(&mut msg, KIND_KEY, skeleton.kind.as_str());
    if let Some(branch) = &skeleton.branch {
        trailer(&mut msg, BRANCH_KEY, branch);
    }
    trailer(&mut msg, BASE_KEY, &oid_value(skeleton.base));
    trailer(&mut msg, PREV_KEY, &oid_value(skeleton.prev));
    trailer(
        &mut msg,
        PREV_BRANCH_KEY,
        &oid_value(skeleton.prev_on_branch),
    );
    trailer(
        &mut msg,
        PREV_SEGMENT_KEY,
        &match skeleton.prev_segment {
            None | Some(SegmentLink::ChainStart) => NONE.to_string(),
            Some(SegmentLink::At(id)) => id.to_string(),
        },
    );
    trailer(&mut msg, REFS_KEY, &oid_value(skeleton.refs_blob));
    if let Some(session) = &skeleton.session {
        trailer(&mut msg, SESSION_KEY, session);
    }
    msg
}

fn trailer(msg: &mut String, key: &str, value: &str) {
    msg.push_str(key);
    msg.push_str(": ");
    msg.push_str(value);
    msg.push('\n');
}

fn oid_value(id: Option<gix::ObjectId>) -> String {
    id.map_or_else(|| NONE.to_string(), |id| id.to_string())
}

/// Read the skeleton back. `None` means the message carries no `fufu-kind`
/// trailer at all — the one guard that separates an op commit from the
/// record commit hanging off it, and from any commit fufu did not write.
pub fn parse(message: &str) -> Option<Skeleton> {
    let kind = OpKind::from_str(value_of(message, KIND_KEY)?)?;
    Some(Skeleton {
        kind,
        branch: value_of(message, BRANCH_KEY).map(str::to_string),
        base: oid_of(message, BASE_KEY),
        prev: oid_of(message, PREV_KEY),
        prev_on_branch: oid_of(message, PREV_BRANCH_KEY),
        // Absent and `none` differ here: absent is an op written before the
        // link existed (walk it the slow way), `none` is a positive claim
        // that nothing precedes this segment (stop).
        prev_segment: value_of(message, PREV_SEGMENT_KEY).and_then(|v| {
            if v == NONE {
                Some(SegmentLink::ChainStart)
            } else {
                gix::ObjectId::from_hex(v.as_bytes())
                    .ok()
                    .map(SegmentLink::At)
            }
        }),
        refs_blob: oid_of(message, REFS_KEY),
        session: value_of(message, SESSION_KEY).map(str::to_string),
    })
}

/// The subject line, which is the whole message up to the first newline.
pub fn subject_of(message: &str) -> &str {
    message.lines().next().unwrap_or("").trim_end()
}

/// The oversize files this op's capture dropped, in written order.
pub fn skipped_of(message: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_block = false;
    for line in message.lines() {
        if line == "Skipped (fufu.maxFileSize):" {
            in_block = true;
            continue;
        }
        if in_block {
            if line.is_empty() {
                break;
            }
            out.push(line.to_string());
        }
    }
    out
}

fn value_of<'m>(message: &'m str, key: &str) -> Option<&'m str> {
    message.lines().find_map(|line| {
        let rest = line.strip_prefix(key)?;
        Some(rest.strip_prefix(": ")?.trim())
    })
}

fn oid_of(message: &str, key: &str) -> Option<gix::ObjectId> {
    let value = value_of(message, key)?;
    (value != NONE)
        .then(|| gix::ObjectId::from_hex(value.as_bytes()).ok())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(byte: u8) -> gix::ObjectId {
        gix::ObjectId::from_bytes_or_panic(&[byte; 20])
    }

    fn full() -> Skeleton {
        Skeleton {
            kind: OpKind::Op,
            branch: Some("feat/x".into()),
            base: Some(oid(0x11)),
            prev: Some(oid(0x22)),
            prev_on_branch: Some(oid(0x33)),
            prev_segment: Some(SegmentLink::At(oid(0x44))),
            refs_blob: Some(oid(0x55)),
            session: Some("agent-7".into()),
        }
    }

    #[test]
    fn skeleton_round_trips() {
        let msg = build("commit: land the parser", &[], &full());
        assert_eq!(parse(&msg).as_ref(), Some(&full()));
        assert_eq!(subject_of(&msg), "commit: land the parser");
    }

    #[test]
    fn empty_skeleton_round_trips_as_nones() {
        let skeleton = Skeleton::new(OpKind::Capture);
        let msg = build("manual", &[], &skeleton);
        let back = parse(&msg).expect("kind trailer present");
        assert_eq!(back.kind, OpKind::Capture);
        assert_eq!(back.base, None);
        assert_eq!(back.prev, None);
        assert_eq!(back.prev_on_branch, None);
        assert_eq!(back.refs_blob, None);
        assert_eq!(
            back.prev_segment,
            Some(SegmentLink::ChainStart),
            "`none` is a positive claim that nothing precedes this segment"
        );
    }

    #[test]
    fn skipped_block_survives_the_trailer_paragraph() {
        let skipped = vec!["big.bin".to_string(), "huge/blob.iso".to_string()];
        let msg = build("manual", &skipped, &Skeleton::new(OpKind::Capture));
        assert_eq!(skipped_of(&msg), skipped);
        assert_eq!(parse(&msg).unwrap().kind, OpKind::Capture);
        assert!(
            msg.contains("blob.iso\n\nfufu-kind: capture\n"),
            "one blank line separates the block from the trailers: {msg:?}"
        );
    }

    #[test]
    fn a_message_without_the_kind_trailer_is_not_an_op() {
        assert!(
            parse("record").is_none(),
            "record commits carry no skeleton"
        );
        assert!(parse("manual\n\nfufu-base: none\n").is_none());
        assert!(parse("manual\n\nfufu-kind: nonsense\n").is_none());
    }

    #[test]
    fn subject_is_cleaned_and_capped() {
        let msg = build(
            "  claude\tacted   twice ",
            &[],
            &Skeleton::new(OpKind::Capture),
        );
        assert_eq!(subject_of(&msg), "claude acted twice");
        let long = "x".repeat(MAX_SUBJECT + 50);
        let msg = build(&long, &[], &Skeleton::new(OpKind::Capture));
        assert_eq!(subject_of(&msg).chars().count(), MAX_SUBJECT);
    }
}
