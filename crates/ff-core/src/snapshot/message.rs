//! Snapshot commit messages: a provenance subject plus an optional body
//! listing oversize-skipped files and/or the segment skip-link trailer. No
//! timestamp or branch in the message — the commit and the ref already carry
//! those.
//!
//! The trailer lives only in the body of commits fufu itself writes under
//! `refs/fufu/snap/*` and `refs/fufu/trash/*`. It never touches a commit the
//! user made, so deleting those refs (abandoning fufu) leaves no trace of it
//! anywhere in the user's own history — the same disposability the rest of
//! fufu's state promises.

/// Subjects are capped so `log --oneline`-style rendering stays sane.
pub const MAX_SUBJECT: usize = 120;

/// The message-body trailer key for the segment skip-link: the newest
/// snapshot of the previous segment (see `evolog::segment_anchors`). A
/// trailer rather than a third parent — parent order is already load-bearing
/// (prev snapshot, base edge) — and a message trailer stays legible to plain
/// git, unlike a note or a side file.
const SEGMENT_PREV_KEY: &str = "fufu-segment-prev";

/// What a snapshot knows about the segment before its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentPrev {
    /// This snapshot's segment is the chain's first: there is nothing earlier.
    ChainStart,
    /// The newest snapshot of the previous segment.
    At(gix::ObjectId),
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

/// Build the full commit message: subject, plus a body listing skipped files
/// and/or the segment skip-link trailer. Either section, both, or neither may
/// be present; the trailer is always last, so `rewrite_segment_prev` can find
/// and replace it without knowing whether a skip block precedes it.
pub fn build(subject: &str, skipped: &[String], segment_prev: Option<SegmentPrev>) -> String {
    let subject = clean_subject(subject, MAX_SUBJECT);
    let mut msg = subject;
    if !skipped.is_empty() {
        msg.push_str("\n\nSkipped (fufu.maxFileSize):\n");
        for path in skipped {
            msg.push_str(path);
            msg.push('\n');
        }
    }
    match segment_prev {
        Some(prev) => append_segment_prev(msg, prev),
        None => msg,
    }
}

/// Append the trailer paragraph, always separated by a literal `"\n\n"` —
/// never reusing a newline the caller's content happened to already end
/// with. That is what makes the separator unambiguous to strip back off
/// later in `rewrite_segment_prev`: the two newlines immediately before the
/// key are always exactly the ones this function added, so finding them
/// finds exactly the boundary this function drew.
fn append_segment_prev(mut msg: String, prev: SegmentPrev) -> String {
    msg.push_str("\n\n");
    msg.push_str(SEGMENT_PREV_KEY);
    msg.push_str(": ");
    match prev {
        SegmentPrev::ChainStart => msg.push_str("none"),
        SegmentPrev::At(oid) => msg.push_str(&oid.to_string()),
    }
    msg.push('\n');
    msg
}

/// Parse the segment skip-link out of a snapshot commit's message, if
/// present. Absent on a chain written before this trailer existed — that
/// chain heals from the tip down as new, pointered captures land on top of
/// it, and until then the anchor walk just falls back to a plain walk from
/// wherever the pointer runs out.
pub fn parse_segment_prev(message: &str) -> Option<SegmentPrev> {
    message.lines().find_map(|line| {
        let rest = line.strip_prefix(SEGMENT_PREV_KEY)?;
        let value = rest.strip_prefix(": ")?;
        let value = value.trim();
        if value == "none" {
            Some(SegmentPrev::ChainStart)
        } else {
            gix::ObjectId::from_hex(value.as_bytes())
                .ok()
                .map(SegmentPrev::At)
        }
    })
}

/// Strip the trailer paragraph (and its separating blank line) out of a
/// message, if present, and reappend it pointing at `new` — or leave it off
/// when `new` is `None`. Used only by `trim`, which must relink a surviving
/// snapshot's trailer to a rewritten id or drop it when the target did not
/// survive; a message with no trailer round-trips unchanged, which is what
/// keeps trim's byte-preservation promise intact for every survivor that
/// never carried one.
pub fn rewrite_segment_prev(message: &str, new: Option<SegmentPrev>) -> String {
    let marker = format!("\n\n{SEGMENT_PREV_KEY}: ");
    let stripped = match message.find(&marker) {
        Some(idx) => message[..idx].to_string(),
        None => message.to_string(),
    };
    match new {
        Some(prev) => append_segment_prev(stripped, prev),
        None => stripped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_and_caps() {
        assert_eq!(clean_subject("  a\t\nb   c ", 120), "a b c");
        let long = "x".repeat(200);
        let capped = clean_subject(&long, 120);
        assert_eq!(capped.chars().count(), 120);
        assert!(capped.ends_with('…'));
    }

    #[test]
    fn body_lists_skips() {
        let msg = build("manual", &["big.bin".into()], None);
        assert_eq!(msg, "manual\n\nSkipped (fufu.maxFileSize):\nbig.bin\n");
    }

    fn oid(byte: u8) -> gix::ObjectId {
        gix::ObjectId::from_bytes_or_panic(&[byte; 20])
    }

    #[test]
    fn trailer_alone_round_trips() {
        let msg = build("manual", &[], Some(SegmentPrev::At(oid(0xab))));
        assert_eq!(
            msg,
            format!("manual\n\nfufu-segment-prev: {}\n", "ab".repeat(20))
        );
        assert_eq!(parse_segment_prev(&msg), Some(SegmentPrev::At(oid(0xab))));
    }

    #[test]
    fn trailer_after_skip_block_round_trips() {
        let msg = build(
            "manual",
            &["big.bin".into()],
            Some(SegmentPrev::At(oid(0xcd))),
        );
        assert_eq!(
            msg,
            format!(
                "manual\n\nSkipped (fufu.maxFileSize):\nbig.bin\n\n\nfufu-segment-prev: {}\n",
                "cd".repeat(20)
            )
        );
        assert_eq!(parse_segment_prev(&msg), Some(SegmentPrev::At(oid(0xcd))));
    }

    #[test]
    fn chain_start_round_trips() {
        let msg = build("manual", &[], Some(SegmentPrev::ChainStart));
        assert_eq!(msg, "manual\n\nfufu-segment-prev: none\n");
        assert_eq!(parse_segment_prev(&msg), Some(SegmentPrev::ChainStart));
    }

    #[test]
    fn chain_start_with_skip_block() {
        let msg = build("manual", &["big.bin".into()], Some(SegmentPrev::ChainStart));
        assert_eq!(
            msg,
            "manual\n\nSkipped (fufu.maxFileSize):\nbig.bin\n\n\nfufu-segment-prev: none\n"
        );
        assert_eq!(parse_segment_prev(&msg), Some(SegmentPrev::ChainStart));
    }

    #[test]
    fn no_trailer_parses_to_none() {
        assert_eq!(parse_segment_prev("manual"), None);
        assert_eq!(
            parse_segment_prev("manual\n\nSkipped (fufu.maxFileSize):\nbig.bin\n"),
            None
        );
    }

    #[test]
    fn garbage_trailer_value_parses_to_none() {
        assert_eq!(
            parse_segment_prev("manual\n\nfufu-segment-prev: not-a-real-id\n"),
            None
        );
    }

    #[test]
    fn rewrite_relinks_or_drops_and_no_trailer_is_a_no_op() {
        let with_trailer = build(
            "manual",
            &["big.bin".into()],
            Some(SegmentPrev::At(oid(0x11))),
        );

        // Relink to a different target.
        let relinked = rewrite_segment_prev(&with_trailer, Some(SegmentPrev::At(oid(0x22))));
        assert_eq!(
            relinked,
            build(
                "manual",
                &["big.bin".into()],
                Some(SegmentPrev::At(oid(0x22)))
            )
        );

        // Drop entirely.
        let dropped = rewrite_segment_prev(&with_trailer, None);
        assert_eq!(dropped, build("manual", &["big.bin".into()], None));

        // A message with no trailer at all round-trips byte for byte —
        // trim's byte-preservation promise for ordinary survivors.
        let plain = build("manual", &["big.bin".into()], None);
        assert_eq!(rewrite_segment_prev(&plain, None), plain);
    }
}
