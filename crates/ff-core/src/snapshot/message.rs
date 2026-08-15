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

/// The message-body trailer key for the session name: a named span of the
/// capture chain. Snapshot commits taken while a session is open carry this
/// trailer so the reading half can group them.
const SESSION_KEY: &str = "fufu-session";

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
/// and/or trailers. Either section, both, or neither may be present. The
/// `fufu-segment-prev` trailer is always last, so `rewrite_segment_prev` can
/// find and replace it without knowing whether a skip block or a
/// `fufu-session` trailer precedes it.
pub fn build(
    subject: &str,
    skipped: &[String],
    session: Option<&str>,
    segment_prev: Option<SegmentPrev>,
) -> String {
    let subject = clean_subject(subject, MAX_SUBJECT);
    let mut msg = subject;
    if !skipped.is_empty() {
        msg.push_str("\n\nSkipped (fufu.maxFileSize):\n");
        for path in skipped {
            msg.push_str(path);
            msg.push('\n');
        }
    }
    // Session trailer comes before segment-prev. When both are present,
    // session gets its own paragraph so that rewrite_segment_prev (which
    // strips from "\n\nfufu-segment-prev: ") keeps working untouched.
    if let Some(sess) = session {
        msg.push_str("\n\n");
        msg.push_str(SESSION_KEY);
        msg.push_str(": ");
        msg.push_str(sess);
        msg.push('\n');
    }
    if let Some(prev) = segment_prev {
        // append_segment_prev always adds "\n\n" before the key. If the
        // message already ends with "\n" (skip block or session trailer),
        // drop one to avoid a triple-newline while keeping the exact "\n\n"
        // separator that rewrite_segment_prev relies on.
        if msg.ends_with('\n') {
            msg.pop();
        }
        append_segment_prev(msg, prev)
    } else {
        msg
    }
}

/// Append just the segment-prev trailer line (no blank-line separator).
/// Used internally by `append_segment_prev`.
fn append_segment_prev_line(msg: &mut String, prev: SegmentPrev) {
    msg.push_str(SEGMENT_PREV_KEY);
    msg.push_str(": ");
    match prev {
        SegmentPrev::ChainStart => msg.push_str("none"),
        SegmentPrev::At(oid) => msg.push_str(&oid.to_string()),
    }
    msg.push('\n');
}

/// Append the trailer paragraph, always separated by a literal `"\n\n"` —
/// never reusing a newline the caller's content happened to already end
/// with. That is what makes the separator unambiguous to strip back off
/// later in `rewrite_segment_prev`: the two newlines immediately before the
/// key are always exactly the ones this function added, so finding them
/// finds exactly the boundary this function drew.
///
/// Called only by `rewrite_segment_prev` — `build` uses
/// `append_segment_prev_line` instead so the session trailer can share
/// the same paragraph.
fn append_segment_prev(mut msg: String, prev: SegmentPrev) -> String {
    msg.push_str("\n\n");
    append_segment_prev_line(&mut msg, prev);
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
        None => {
            // The "\n\n" separator consumed the trailing newline from any body
            // content (skip block or session trailer). Restore it so the
            // result matches what build() would produce without a trailer.
            if !stripped.ends_with('\n') {
                let mut s = stripped;
                s.push('\n');
                s
            } else {
                stripped
            }
        }
    }
}

/// The session name a snapshot commit message carries, if any.
pub fn session_of(message: &str) -> Option<&str> {
    message.lines().find_map(|line| {
        let rest = line.strip_prefix(SESSION_KEY)?;
        rest.strip_prefix(": ")
    })
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
        let msg = build("manual", &["big.bin".into()], None, None);
        assert_eq!(msg, "manual\n\nSkipped (fufu.maxFileSize):\nbig.bin\n");
    }

    fn oid(byte: u8) -> gix::ObjectId {
        gix::ObjectId::from_bytes_or_panic(&[byte; 20])
    }

    #[test]
    fn trailer_alone_round_trips() {
        let msg = build("manual", &[], None, Some(SegmentPrev::At(oid(0xab))));
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
            None,
            Some(SegmentPrev::At(oid(0xcd))),
        );
        assert_eq!(
            msg,
            format!(
                "manual\n\nSkipped (fufu.maxFileSize):\nbig.bin\n\nfufu-segment-prev: {}\n",
                "cd".repeat(20)
            )
        );
        assert_eq!(parse_segment_prev(&msg), Some(SegmentPrev::At(oid(0xcd))));
    }

    #[test]
    fn chain_start_round_trips() {
        let msg = build("manual", &[], None, Some(SegmentPrev::ChainStart));
        assert_eq!(msg, "manual\n\nfufu-segment-prev: none\n");
        assert_eq!(parse_segment_prev(&msg), Some(SegmentPrev::ChainStart));
    }

    #[test]
    fn chain_start_with_skip_block() {
        let msg = build(
            "manual",
            &["big.bin".into()],
            None,
            Some(SegmentPrev::ChainStart),
        );
        assert_eq!(
            msg,
            "manual\n\nSkipped (fufu.maxFileSize):\nbig.bin\n\nfufu-segment-prev: none\n"
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
            None,
            Some(SegmentPrev::At(oid(0x11))),
        );

        // Relink to a different target.
        let relinked = rewrite_segment_prev(&with_trailer, Some(SegmentPrev::At(oid(0x22))));
        assert_eq!(
            relinked,
            build(
                "manual",
                &["big.bin".into()],
                None,
                Some(SegmentPrev::At(oid(0x22)))
            )
        );

        // Drop entirely.
        let dropped = rewrite_segment_prev(&with_trailer, None);
        assert_eq!(dropped, build("manual", &["big.bin".into()], None, None));

        // A message with no trailer at all round-trips byte for byte —
        // trim's byte-preservation promise for ordinary survivors.
        let plain = build("manual", &["big.bin".into()], None, None);
        assert_eq!(rewrite_segment_prev(&plain, None), plain);
    }

    // --- session trailer tests ---

    #[test]
    fn session_trailer_precedes_segment_prev() {
        let msg = build(
            "manual",
            &[],
            Some("refactor-parser"),
            Some(SegmentPrev::ChainStart),
        );
        // fufu-session appears before fufu-segment-prev in the message.
        let session_pos = msg.find("fufu-session:").expect("session trailer present");
        let segment_pos = msg
            .find("fufu-segment-prev:")
            .expect("segment-prev trailer present");
        assert!(
            session_pos < segment_pos,
            "session trailer must precede segment-prev"
        );
        // segment-prev is still the last trailer line.
        assert!(msg.ends_with("fufu-segment-prev: none\n"));
    }

    #[test]
    fn segment_prev_rewrite_survives_a_session_trailer() {
        let msg = build(
            "manual",
            &[],
            Some("refactor-parser"),
            Some(SegmentPrev::At(oid(0x11))),
        );
        // Rewrite the segment-prev to a new sha.
        let rewritten = rewrite_segment_prev(&msg, Some(SegmentPrev::At(oid(0x22))));
        // The session trailer is still present.
        assert_eq!(session_of(&rewritten), Some("refactor-parser"));
        // The new sha landed.
        assert_eq!(
            parse_segment_prev(&rewritten),
            Some(SegmentPrev::At(oid(0x22)))
        );
    }

    #[test]
    fn session_of_reads_the_trailer() {
        // Round-trip: build then read.
        let msg = build(
            "manual",
            &[],
            Some("refactor-parser"),
            Some(SegmentPrev::ChainStart),
        );
        assert_eq!(session_of(&msg), Some("refactor-parser"));

        // No session trailer returns None.
        let no_session = build("manual", &[], None, Some(SegmentPrev::ChainStart));
        assert_eq!(session_of(&no_session), None);

        // Message with no trailer at all returns None.
        assert_eq!(session_of("manual"), None);
    }

    #[test]
    fn session_only_no_segment_prev() {
        // Session trailer alone (no segment-prev) works.
        let msg = build("manual", &[], Some("refactor-parser"), None);
        assert_eq!(session_of(&msg), Some("refactor-parser"));
        assert_eq!(parse_segment_prev(&msg), None);
    }
}
