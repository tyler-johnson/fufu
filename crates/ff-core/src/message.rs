//! One commit message, split the way git splits it.
//!
//! The subject is what every row shows and the body is what `ff show` and
//! `ff log --body` print under it. Both come from one rule so the subject a
//! row shows is the subject the header shows, and the pending description
//! splits the same way the commit it becomes will.

use gix::bstr::ByteSlice;
use gix::objs::commit::MessageRef;

/// A commit message as its two halves: the subject, git's summary of the
/// title, and the body after the first blank line with trailing whitespace
/// trimmed, empty when there is none. gix's split, so the subject here is
/// the subject everywhere.
pub fn split(raw: &[u8]) -> (String, String) {
    let msg = MessageRef::from_bytes(raw);
    let subject = msg.summary().to_string();
    let body = msg
        .body
        .map(|b| b.to_str_lossy().trim_end().to_string())
        .unwrap_or_default();
    (subject, body)
}

#[cfg(test)]
mod tests {
    use super::split;

    #[test]
    fn a_one_line_message_has_no_body() {
        assert_eq!(split(b"one"), ("one".into(), String::new()));
        assert_eq!(split(b"one\n"), ("one".into(), String::new()));
    }

    #[test]
    fn the_body_keeps_its_paragraphs() {
        let (subject, body) = split(b"subject\n\nfirst para\nsecond line\n\nsecond para\n");
        assert_eq!(subject, "subject");
        assert_eq!(body, "first para\nsecond line\n\nsecond para");
    }

    #[test]
    fn a_trailing_newline_is_trimmed() {
        assert_eq!(split(b"s\n\nb\n\n"), ("s".into(), "b".into()));
    }

    #[test]
    fn a_crlf_separator_splits_too() {
        assert_eq!(split(b"s\r\n\r\nb\r\n"), ("s".into(), "b".into()));
    }

    #[test]
    fn a_two_line_title_folds_into_one_subject() {
        let (subject, body) = split(b"first\nsecond\n\nbody");
        assert_eq!(subject, "first second");
        assert_eq!(body, "body");
    }
}
