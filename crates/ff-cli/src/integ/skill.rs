//! The shipped skill: fufu's own manual, for the clients that read one.
//!
//! The once-per-session briefing is budgeted, because it is context every
//! session pays for whether or not it is needed. This is the other half of
//! that bargain — the advanced surface, on a shelf, costing nothing until
//! a client decides the situation calls for it. One text, verbatim, for
//! every client that reads skills: they agree on the file's name and on
//! its frontmatter, so there is nothing per-vendor to adapt.
//!
//! Delivery is the same story `claude.rs` already tells: a directory fufu
//! owns outright, written whole and removed whole, with no foreign content
//! to preserve. Claude takes it inside the plugin it already owns; Codex
//! takes a directory of its own beside the settings file it does not.
//!
//! The second half of the file is the same idea for a declared extension's
//! own skill: the manifest names the files, `sources` reads them from
//! wherever it said to look, and `write_sources` lands them under a
//! directory beside fufu's own — one more directory this fufu owns
//! outright, keyed by the extension's name instead of fixed to `fufu`.

use std::path::{Path, PathBuf};

use ff_core::{Error, Result};

use super::{Mechanism, Wiring};

/// The skill, as both clients read it. Kept as Markdown rather than a Rust
/// string so it stays a document: the frontmatter has to be the first
/// bytes of the file, and prose this long is not a literal anybody would
/// edit twice.
pub const SKILL: &str = include_str!("skill.md");

/// The directory fufu owns inside a client's skills location, and the one
/// file in it. Both are the clients' conventions rather than fufu's.
pub const NAME: &str = "fufu";
const FILE: &str = "SKILL.md";

/// What the briefing adds where the skill actually landed. It is appended
/// by the runtime rather than baked into the notice, because a client with
/// no skill must not be told to read one.
pub const LINE: &str = "\nRecovering earlier file state, rewriting commits that have already closed, \
                        and resolving conflicts are in the `fufu` skill — read it before reaching \
                        for `ff git`.\n";

pub fn path(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

pub fn write(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).map_err(Error::repo)?;
    std::fs::write(path(dir), SKILL).map_err(Error::repo)
}

/// Answers whether there was anything to remove, so a caller can report
/// honestly instead of claiming a change it did not make.
pub fn remove(dir: &Path) -> Result<bool> {
    if !dir.exists() {
        return Ok(false);
    }
    std::fs::remove_dir_all(dir).map_err(Error::repo)?;
    Ok(true)
}

/// Whether there is a skill here at all, which is the question the
/// briefing asks: a manual an older fufu wrote still reads, and pointing
/// an agent at it beats pointing it nowhere.
pub fn installed(dir: &Path) -> bool {
    path(dir).is_file()
}

/// A skill this binary would write differently is `Partial` rather than
/// missing: it is on disk and it works, it simply describes a fufu that has
/// moved. That is a repair `ff doctor --fix` makes and never an outage.
pub fn wiring(dir: &Path) -> Wiring {
    match std::fs::read_to_string(path(dir)) {
        Ok(text) if text == SKILL => Wiring::Wired {
            mechanism: Mechanism::Plugin,
            at: dir.to_path_buf(),
        },
        Ok(_) => Wiring::Partial {
            missing: "an older fufu wrote it".into(),
            at: dir.to_path_buf(),
        },
        Err(_) => Wiring::NotWired,
    }
}

// ---- a declared extension's own skill files --------------------------------

/// Past this, a manifest-named skill file is treated exactly like one that
/// could not be read at all. An extension's skill is meant to be a manual,
/// not an arbitrary blob, and a cap here is what keeps a hook install from
/// shipping one into a client's directory sight unseen.
const MAX_SOURCE_LEN: u64 = 8 * 1024 * 1024;

/// One file read from wherever a manifest's `skills` entry named it, ready
/// to write under a client's `skills/<name>/` under its own basename — the
/// source file's own name, since a manifest may name more than one and a
/// client tells them apart by it.
pub struct SourceFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Every skill file a declared extension names, read from wherever its
/// manifest said to find them: absolute, or relative to the directory its
/// binary lives in, exactly as the manifest documents the field.
///
/// A path that does not exist, is not a plain file, cannot be read, or is
/// larger than [`MAX_SOURCE_LEN`] is left out rather than failing the whole
/// call — one bad path must not cost every other file its place, and an
/// extension with one fewer skill file is a smaller loss than a hook
/// install that stops partway through every client. An absolute path
/// resolves whether or not the extension's binary is still on PATH; a
/// relative one needs it, on the rule the manifest states the field by.
pub fn sources(declared: &crate::registry::Declared) -> Vec<SourceFile> {
    let base = declared
        .resolve()
        .and_then(|binary| binary.parent().map(Path::to_path_buf));
    declared
        .manifest
        .skills
        .iter()
        .filter_map(|named| source(base.as_deref(), named))
        .collect()
}

fn source(base: Option<&Path>, named: &str) -> Option<SourceFile> {
    let named = Path::new(named);
    let resolved = if named.is_absolute() {
        named.to_path_buf()
    } else {
        base?.join(named)
    };
    let name = resolved.file_name()?.to_str()?.to_string();
    let meta = std::fs::metadata(&resolved).ok()?;
    if !meta.is_file() || meta.len() > MAX_SOURCE_LEN {
        return None;
    }
    let bytes = std::fs::read(&resolved).ok()?;
    Some(SourceFile { name, bytes })
}

/// Write a declared extension's skill files into `dir`, replacing whatever
/// was there — the same refresh a reinstall gives fufu's own skill.
/// Answers how many files landed, so a caller can report a directory it
/// actually wrote into and say nothing about one it did not.
///
/// `dir` is removed first and rewritten from scratch rather than merged,
/// because a file the manifest no longer names must not linger: refreshing
/// is supposed to mean the directory now holds exactly what the manifest
/// says, not that plus whatever an earlier install left behind.
pub fn write_sources(dir: &Path, declared: &crate::registry::Declared) -> Result<usize> {
    let files = sources(declared);
    remove(dir)?;
    if files.is_empty() {
        return Ok(0);
    }
    std::fs::create_dir_all(dir).map_err(Error::repo)?;
    for file in &files {
        std::fs::write(dir.join(&file.name), &file.bytes).map_err(Error::repo)?;
    }
    Ok(files.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_skill_leads_with_its_frontmatter() {
        let mut lines = SKILL.lines();
        assert_eq!(lines.next(), Some("---"));
        let head: Vec<&str> = lines.take_while(|line| *line != "---").collect();
        assert!(
            head.iter().any(|line| line.starts_with("name: ")),
            "a client reads the skill by its name: {head:?}"
        );
        assert!(
            head.iter().any(|line| line.starts_with("description: ")),
            "the description is the whole trigger — without it the skill never loads"
        );
    }

    #[test]
    fn it_round_trips_through_a_directory() {
        let home = tempfile::TempDir::new().unwrap();
        let dir = home.path().join("skills/fufu");
        assert_eq!(wiring(&dir), Wiring::NotWired);
        assert!(!installed(&dir));

        write(&dir).unwrap();
        assert!(matches!(wiring(&dir), Wiring::Wired { .. }));
        assert!(installed(&dir));

        std::fs::write(path(&dir), "an older fufu wrote this").unwrap();
        assert!(
            matches!(wiring(&dir), Wiring::Partial { .. }),
            "text that is not what this binary ships is a repair, not a hole"
        );
        assert!(
            installed(&dir),
            "it still reads, so the briefing still names it"
        );

        assert!(remove(&dir).unwrap());
        assert!(!remove(&dir).unwrap(), "nothing left to take");
        assert_eq!(wiring(&dir), Wiring::NotWired);
    }

    mod extension_sources {
        use super::*;

        /// A declared extension naming exactly these skill files. The path
        /// on the record is one nothing on any real PATH answers to, so
        /// `resolve` finds nothing — the same state a binary that has left
        /// PATH is in, and the case a relative skill entry cannot resolve
        /// through.
        fn declared(skills: Vec<String>) -> crate::registry::Declared {
            let manifest = crate::manifest::parse(serde_json::json!({
                "name": "skillsrctest",
                "version": "0.1.0",
                "contract": 1,
                "verbs": [{"name": "go", "read_only": true}],
                "undoable": true,
                "skills": skills,
            }))
            .expect("a manifest the page types");
            crate::registry::Declared {
                manifest,
                path: PathBuf::from("/nonexistent/ff-skillsrctest"),
                declared_at: 0,
            }
        }

        #[test]
        fn an_absolute_path_is_read_by_its_basename() {
            let dir = tempfile::TempDir::new().unwrap();
            let file = dir.path().join("tower.md");
            std::fs::write(&file, "the manual").unwrap();

            let files = sources(&declared(vec![file.display().to_string()]));
            assert_eq!(files.len(), 1);
            assert_eq!(files[0].name, "tower.md");
            assert_eq!(files[0].bytes, b"the manual");
        }

        #[test]
        fn a_missing_file_is_left_out_not_an_error() {
            let dir = tempfile::TempDir::new().unwrap();
            let missing = dir.path().join("nope.md");
            assert!(sources(&declared(vec![missing.display().to_string()])).is_empty());
        }

        #[test]
        fn an_oversized_file_is_left_out() {
            let dir = tempfile::TempDir::new().unwrap();
            let file = dir.path().join("huge.md");
            std::fs::write(&file, vec![b'x'; (MAX_SOURCE_LEN + 1) as usize]).unwrap();
            assert!(sources(&declared(vec![file.display().to_string()])).is_empty());
        }

        /// A relative entry is resolved against the binary's own directory,
        /// and there is no binary here to resolve against.
        #[test]
        fn a_relative_path_needs_a_resolvable_binary() {
            assert!(sources(&declared(vec!["skills/tower.md".into()])).is_empty());
        }

        #[test]
        fn every_entry_lands_in_the_manifests_own_order() {
            let dir = tempfile::TempDir::new().unwrap();
            let a = dir.path().join("a.md");
            let b = dir.path().join("b.md");
            std::fs::write(&a, "a").unwrap();
            std::fs::write(&b, "b").unwrap();

            let files = sources(&declared(vec![
                a.display().to_string(),
                b.display().to_string(),
            ]));
            let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
            assert_eq!(names, ["a.md", "b.md"]);
        }

        /// A reinstall refreshes: the directory ends up holding exactly what
        /// the manifest names now, not that plus whatever an earlier run
        /// left behind, and nothing readable at all is the directory gone
        /// rather than lingering with stale content.
        #[test]
        fn write_sources_refreshes_and_drops_what_is_no_longer_named() {
            let home = tempfile::TempDir::new().unwrap();
            let src = tempfile::TempDir::new().unwrap();
            let dir = home.path().join("skills/tower");

            let a = src.path().join("a.md");
            let b = src.path().join("b.md");
            std::fs::write(&a, "a").unwrap();
            std::fs::write(&b, "b").unwrap();

            let n = write_sources(
                &dir,
                &declared(vec![a.display().to_string(), b.display().to_string()]),
            )
            .unwrap();
            assert_eq!(n, 2);
            assert!(dir.join("a.md").exists());
            assert!(dir.join("b.md").exists());

            let n = write_sources(&dir, &declared(vec![a.display().to_string()])).unwrap();
            assert_eq!(n, 1);
            assert!(dir.join("a.md").exists());
            assert!(
                !dir.join("b.md").exists(),
                "a file no longer named must not linger"
            );

            std::fs::remove_file(&a).unwrap();
            let n = write_sources(&dir, &declared(vec![a.display().to_string()])).unwrap();
            assert_eq!(n, 0);
            assert!(!dir.exists(), "nothing readable left is the directory gone");
        }
    }
}
