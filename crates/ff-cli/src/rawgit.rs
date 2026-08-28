//! What a command says about git, and what fufu would have done instead.
//!
//! One table and one guard, shared by the two places raw git reaches fufu:
//! `ff git <args…>`, which arrives as clean argv through the recommended
//! alias, and a `git …` string inside an agent's shell tool, which arrives
//! through the `BeforeTool` hook as one unparsed line.
//!
//! The table is the git words fufu has a verb to name. That is what makes
//! it self-limiting: `git apply`, `git am`, `git bisect`, `git submodule`
//! and `git gc` are writes with no fufu answer, so they are not in the
//! table and are never touched. fufu only corrects what it can answer.
//!
//! [`classify_command`] is a guard before it is a parser. Shell strings are
//! not safe to interpret, so anything past one plain `git <word> …`
//! invocation answers [`Shape::Ambiguous`] and is left alone. Ambiguity
//! fails open in both directions: nothing is denied and nothing is coached.
//! The capture already happened either way, so the net is intact.

use std::ffi::OsString;

/// A git word fufu has an answer for: what git spells it, what fufu spells
/// it, and one line saying why the fufu verb is the one to reach for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Word {
    pub git: &'static str,
    pub ff: &'static str,
    pub why: &'static str,
}

impl Word {
    /// Whether fufu's answer *is* the passthrough. `rebase` and `tag` are
    /// the two: fufu has no verb of its own, and what it has to say is that
    /// `ff git <word>` runs the real thing capture-first. Naming that at
    /// somebody who already typed `ff git rebase` would be answering them
    /// with their own command line, so the alias path stays quiet on these
    /// and only the raw-git path hears about them.
    pub fn is_passthrough(&self) -> bool {
        self.ff.starts_with("ff git ")
    }
}

/// What one command turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape<'a> {
    /// Not a git invocation at all.
    NotGit,
    /// git, and nothing fufu wants to say about it.
    Read,
    /// Something fufu does not fully understand. Never corrected.
    Ambiguous,
    /// A git word fufu has a verb for, and the word as it was typed.
    Write(&'static Word, &'a str),
}

/// The git words fufu has verbs for. Nothing outside this list is ever a
/// [`Shape::Write`], which is what bounds both the tally and the refusal.
pub const TABLE: &[Word] = &[
    Word {
        git: "add",
        ff: "ff commit",
        why: "there is no staging area here — ff commit takes the paths directly",
    },
    Word {
        git: "commit",
        ff: "ff commit",
        why: "the working tree is the change, and ff commit closes it onto the log",
    },
    Word {
        git: "switch",
        ff: "ff switch",
        why: "ff switch parks the open change and resumes whatever was parked where you land",
    },
    Word {
        git: "checkout",
        ff: "ff switch",
        why: "git's checkout was two jobs: ff switch moves, ff restore brings files back",
    },
    Word {
        git: "restore",
        ff: "ff restore",
        why: "ff restore brings files back from any point on the operation log, not just HEAD",
    },
    Word {
        git: "reset",
        ff: "ff undo",
        why: "ff undo steps the whole repository back one operation, index and all",
    },
    Word {
        git: "revert",
        ff: "ff op revert",
        why: "ff op revert inverts one operation and leaves the work after it standing",
    },
    Word {
        git: "branch",
        ff: "ff branch",
        why: "ff branch lists and deletes; ff describe -b names the branch you are on",
    },
    Word {
        git: "merge",
        ff: "ff restack --onto",
        why: "fufu replays rather than merges — ff restack --onto puts this branch on top",
    },
    Word {
        git: "rebase",
        ff: "ff git rebase",
        why: "there is no ff rebase yet, and ff git rebase runs the real thing capture-first",
    },
    Word {
        git: "stash",
        ff: "ff switch",
        why: "switching parks the open change, so there is nothing to push and nothing to pop",
    },
    Word {
        git: "pull",
        ff: "ff sync",
        why: "ff sync is the incoming half done properly: fetch, take in, replay onto the base",
    },
    Word {
        git: "push",
        ff: "ff publish",
        why: "ff publish sends this branch under a lease, refusing rather than overwriting",
    },
    Word {
        git: "fetch",
        ff: "ff sync",
        why: "ff sync fetches and replays in one move, with no merge-or-rebase question",
    },
    Word {
        git: "tag",
        ff: "ff git tag",
        why: "tags are git's to make, and ff git tag makes one capture-first",
    },
    Word {
        git: "clone",
        ff: "ff clone",
        why: "ff clone clones and wires the capture floor up in the same move",
    },
    Word {
        git: "init",
        ff: "ff init",
        why: "ff init starts a repository with the operation log already in it",
    },
    Word {
        git: "worktree",
        ff: "ff worktree",
        why: "ff worktree keeps a bay on the operation log, and captures before removing one",
    },
];

/// The characters that end the guard's confidence. Any of them anywhere in
/// the string and the whole thing is [`Shape::Ambiguous`] — this is why
/// `git commit -m "a && b"` fails open, which is the right direction: a
/// message fufu misread is a command it must not comment on.
const SHELL: &[char] = &['&', '|', ';', '\n', '\r', '`', '$', '<', '>', '(', ')'];

/// Look one git word up.
pub fn word(git: &str) -> Option<&'static Word> {
    TABLE.iter().find(|w| w.git == git)
}

/// Whether this word in this form only reads. A few of the table's words
/// have a bare form that asks a question rather than changing anything, and
/// a question is not something to correct.
fn reads(git: &str, rest: &[&str]) -> bool {
    match git {
        // `git branch`, `git tag`, `git stash` with nothing after them list
        // rather than write.
        "branch" | "tag" | "stash" => rest.is_empty(),
        "worktree" => rest.first() == Some(&"list"),
        _ => false,
    }
}

/// The shared decision, once the subcommand and its tail are known.
fn shape<'a>(git: &'a str, rest: &[&str]) -> Shape<'a> {
    // A word outside the table is one fufu has no answer for. Silence is
    // the answer: correcting what you cannot replace is nagging.
    let Some(entry) = word(git) else {
        return Shape::Read;
    };
    if reads(git, rest) {
        return Shape::Read;
    }
    Shape::Write(entry, git)
}

/// `ff git <args…>` — argv that the shell already split, so there is no
/// string to guard, only the same two rules about what fufu understands: a
/// first token that is a flag is git's own (`-C`, `-c`, `--git-dir`) and
/// changes where the command lands, so it is never corrected.
pub fn classify_argv(args: &[OsString]) -> Shape<'_> {
    let Some((first, tail)) = args.split_first() else {
        return Shape::Read;
    };
    let Some(git) = first.to_str() else {
        return Shape::Ambiguous;
    };
    if git.starts_with('-') {
        return Shape::Ambiguous;
    }
    let rest: Vec<&str> = tail.iter().filter_map(|a| a.to_str()).collect();
    // A tail fufu could not read at all is a tail it cannot tell a read
    // from a write in.
    if rest.len() != tail.len() {
        return Shape::Ambiguous;
    }
    shape(git, &rest)
}

/// A shell command string, as an agent's tool call carries it.
pub fn classify_command(command: &str) -> Shape<'_> {
    if command.contains(SHELL) {
        return Shape::Ambiguous;
    }
    let mut tokens = command.split_whitespace();
    let Some(first) = tokens.next() else {
        return Shape::NotGit;
    };
    // Exactly `git`, and nothing dressed as it: `/usr/bin/git` runs a
    // binary fufu did not resolve, `sudo git` and `GIT_AUTHOR_NAME=x git`
    // put a word in front of it, and each of those is a different command.
    if first != "git" {
        return Shape::NotGit;
    }
    let Some(git) = tokens.next() else {
        return Shape::Read;
    };
    // `git -C path commit` names a repository that is not this one.
    if git.starts_with('-') {
        return Shape::Ambiguous;
    }
    let rest: Vec<&str> = tokens.collect();
    shape(git, &rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn written(shape: Shape<'_>) -> Option<&'static Word> {
        match shape {
            Shape::Write(word, _) => Some(word),
            _ => None,
        }
    }

    #[test]
    fn the_table_names_a_write_and_leaves_a_read_alone() {
        assert_eq!(
            written(classify_command("git commit -m x")).map(|w| w.ff),
            Some("ff commit")
        );
        assert_eq!(classify_command("git status"), Shape::Read);
        assert_eq!(classify_command("git log --oneline"), Shape::Read);
        // The bare forms that only list.
        assert_eq!(classify_command("git branch"), Shape::Read);
        assert_eq!(classify_command("git tag"), Shape::Read);
        assert_eq!(classify_command("git stash"), Shape::Read);
        assert_eq!(classify_command("git worktree list"), Shape::Read);
        // …and the same words when they are doing something.
        assert!(written(classify_command("git branch -d gone")).is_some());
        assert!(written(classify_command("git worktree add ../bay")).is_some());
    }

    /// A write with no fufu answer is not fufu's to comment on.
    #[test]
    fn a_write_fufu_cannot_answer_is_left_alone() {
        for command in [
            "git apply p.diff",
            "git am patch.mbox",
            "git bisect start",
            "git submodule update",
            "git gc --prune=now",
            "git cherry-pick abc123",
        ] {
            assert_ne!(
                classify_command(command),
                Shape::Ambiguous,
                "{command} is plain enough to read"
            );
            assert!(
                written(classify_command(command)).is_none(),
                "{command} has no fufu verb to name"
            );
        }
    }

    /// Anything the guard cannot fully account for fails open.
    #[test]
    fn the_guard_never_writes_on_a_string_it_cannot_read() {
        for command in [
            "git commit && rm -rf x",
            "git commit || true",
            "git commit; echo done",
            "git commit -m \"a && b\"",
            "git log | head",
            "git commit -m $(date)",
            "git commit > out.txt",
            "git -C path commit",
            "git\ncommit",
        ] {
            assert!(
                written(classify_command(command)).is_none(),
                "{command} must not be corrected"
            );
        }
        // Not git at all, however much it looks like it.
        for command in [
            "sudo git push",
            "echo git commit",
            "/usr/bin/git commit",
            "GIT_AUTHOR_NAME=x git commit",
            "cd x",
        ] {
            assert!(
                written(classify_command(command)).is_none(),
                "{command} is not a git invocation fufu resolved"
            );
        }
    }

    #[test]
    fn argv_is_the_same_table_without_the_shell_guard() {
        assert_eq!(
            written(classify_argv(&argv(&["commit", "-m", "a && b"]))).map(|w| w.ff),
            Some("ff commit"),
            "argv is already split — a message is a message"
        );
        assert_eq!(classify_argv(&argv(&["status"])), Shape::Read);
        assert_eq!(classify_argv(&argv(&[])), Shape::Read);
        assert_eq!(
            classify_argv(&argv(&["-C", "path", "commit"])),
            Shape::Ambiguous
        );
    }

    /// The two prose surfaces that answer for a git word — this table and
    /// `cmd::foreign`'s long-form refusals — must name the same verb.
    #[test]
    fn the_foreign_verbs_name_the_same_spelling() {
        type Foreign = fn(&[OsString]) -> ff_core::Result<()>;
        let handled: &[(&str, Foreign)] = &[
            ("checkout", crate::cmd::foreign::checkout),
            ("stash", crate::cmd::foreign::stash),
            ("pull", crate::cmd::foreign::pull),
            ("push", crate::cmd::foreign::push),
            ("rebase", crate::cmd::foreign::rebase),
            ("merge", crate::cmd::foreign::merge),
            ("tag", crate::cmd::foreign::tag),
            ("blame", crate::cmd::foreign::blame),
        ];
        for (git, refuse) in handled {
            let err = refuse(&[]).expect_err("a foreign verb refuses");
            let exits = err.exits().join(" | ");
            match word(git) {
                Some(entry) => assert!(
                    exits.contains(entry.ff),
                    "ff {git} exits with {exits:?}, which never names {:?}",
                    entry.ff
                ),
                // `blame` is the one foreign verb that answers a read, so
                // it is not in the table and must never be corrected.
                None => assert!(
                    written(classify_command(&format!("git {git} src/lib.rs"))).is_none(),
                    "git {git} has no fufu verb and must stay untouched"
                ),
            }
        }
    }

    /// Every spelling the table teaches has to be surface clap still takes,
    /// and surface it does not hide — the same guard the briefing carries.
    #[test]
    fn every_spelling_is_live_documented_surface() {
        use clap::CommandFactory;

        let root = crate::cli::Cli::command();
        for entry in TABLE {
            let tokens: Vec<&str> = entry.ff.split_whitespace().collect();
            assert_eq!(tokens[0], "ff", "{:?} does not name ff", entry.ff);
            let mut cmd = &root;
            let mut rest = &tokens[1..];
            while let Some(sub) = rest.first().and_then(|name| cmd.find_subcommand(name)) {
                assert!(
                    !sub.is_hide_set(),
                    "{:?} names {:?}, which is hidden — retired surface must not be \
                     what fufu corrects somebody onto",
                    entry.ff,
                    sub.get_name()
                );
                cmd = sub;
                rest = &rest[1..];
                // Past `ff git` everything is git's to parse.
                if cmd.get_name() == "git" {
                    break;
                }
            }
            assert!(
                !std::ptr::eq(cmd, &root),
                "{:?} names no verb at all",
                entry.ff
            );
            for flag in rest.iter().filter(|token| token.starts_with('-')) {
                let arg = cmd
                    .get_arguments()
                    .find(|arg| {
                        arg.get_long()
                            .is_some_and(|long| *flag == format!("--{long}"))
                    })
                    .unwrap_or_else(|| {
                        panic!("{:?} passes {flag}, which does not exist", entry.ff)
                    });
                assert!(
                    !arg.is_hide_set(),
                    "{:?} passes {flag}, which is hidden",
                    entry.ff
                );
            }
        }
    }
}
