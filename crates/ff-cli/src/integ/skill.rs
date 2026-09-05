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
//! own skills: the manifest names them, `ext_skills` asks the binary for
//! each one's files through `--ff-skill`, and `write_ext_skill` lands them
//! under a directory beside fufu's own — one more directory this fufu owns
//! outright, keyed by the skill's name instead of fixed to `fufu`.

use std::path::{Path, PathBuf};

use ff_core::{Error, Result};

use super::{Mechanism, Wiring};
use crate::manifest;
use crate::registry::Declared;

/// The skill, as both clients read it. Kept as Markdown rather than a Rust
/// string so it stays a document: the frontmatter has to be the first
/// bytes of the file, and prose this long is not a literal anybody would
/// edit twice.
pub const SKILL: &str = include_str!("skill.md");

/// The directory fufu owns inside a client's skills location, and the one
/// file in it. Both are the clients' conventions rather than fufu's.
pub const NAME: &str = "fufu";
const FILE: &str = manifest::SKILL_FILE;

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

// ---- a declared extension's own skills --------------------------------------

/// One skill an extension produced through `--ff-skill`, ready to write.
pub struct ExtSkill {
    pub name: String,
    pub files: Vec<manifest::SkillFile>,
}

/// Ask a declared extension for every skill its manifest names. Answers the
/// ones that came back whole, and the names that did not with why, so an
/// installer can write the first and say the second.
///
/// One skill failing costs no other its place, and none of them fails the
/// install: an extension with one fewer skill is a smaller loss than a hook
/// install that stops partway through every client. A binary gone from
/// PATH makes every skill a failure, named as such — the handshake needs a
/// binary to run, and a manifest on record is not one.
pub fn ext_skills(declared: &Declared) -> (Vec<ExtSkill>, Vec<(String, Error)>) {
    let name = declared.name();
    let Some(binary) = declared.resolve() else {
        let gone = declared
            .manifest
            .skills
            .iter()
            .map(|skill| {
                (
                    skill.clone(),
                    Error::coded(
                        "extension/not-found",
                        format!("ff-{name} is not on PATH"),
                        vec!["ff doctor".into()],
                    ),
                )
            })
            .collect();
        return (Vec::new(), gone);
    };
    let mut skills = Vec::new();
    let mut failed = Vec::new();
    for skill in &declared.manifest.skills {
        match manifest::ask_skill(&binary, name, skill) {
            Ok(files) => skills.push(ExtSkill {
                name: skill.clone(),
                files,
            }),
            Err(err) => failed.push((skill.clone(), err)),
        }
    }
    (skills, failed)
}

/// `root/<skill.name>`, removed if present, then every file written under
/// it. Answers the directory it wrote.
///
/// Removed first and rewritten from scratch rather than merged, because a
/// file the skill no longer carries must not linger: refreshing means the
/// directory now holds exactly what the binary produced, and not that plus
/// whatever an earlier install left behind.
pub fn write_ext_skill(root: &Path, skill: &ExtSkill) -> Result<PathBuf> {
    let dir = root.join(&skill.name);
    remove(&dir)?;
    for file in &skill.files {
        let path = dir.join(&file.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(Error::repo)?;
        }
        std::fs::write(&path, &file.content).map_err(Error::repo)?;
    }
    Ok(dir)
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
}
