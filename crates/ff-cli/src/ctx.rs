//! `Ctx` — what one invocation settled before any verb ran. The global flags
//! are resolved once here and handed down as a parameter.
//!
//! A set-once global would do for `--session`: it is a cosmetic label, and a
//! command that read a stale one would still do the right work. `--at-op`
//! arrives on this same mechanism, and it decides *which repository* a
//! command is even talking about. Answering that from a global read deep in
//! the call graph makes it a correctness-bearing invisible — nothing in a
//! `run` signature would say the result depends on it. So the mechanism is a
//! parameter from the day the only thing riding it is harmless.

use ff_core::{Error, Result};

use crate::cli::{Cli, Command};

/// Which past state a command was asked to read against, if any.
///
/// One kind per door: `--at-op` takes an operation id and `--at` takes a
/// clock. They are one mechanism with two entrances, so naming both at once
/// is refused rather than ranked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum At {
    Op(String),
    Time(String),
}

#[derive(Debug)]
pub struct Ctx {
    /// Emit the machine envelope instead of prose. The verb's own capability
    /// is already folded in, so consumers only read the flag.
    pub json: bool,
    /// The session this invocation's snapshots are stamped with, if any.
    pub session: Option<String>,
    /// The name every JSON envelope of this invocation carries — payload or
    /// error, emitted from a verb or from main's error handler.
    pub command: &'static str,
    /// The past state this invocation reads against. Settled here so a verb
    /// never has to decide what its two flags mean together.
    pub at: Option<At>,
}

impl Ctx {
    /// Settle the invocation state from the parsed command line. Failures
    /// land before dispatch, so no verb ever runs against a half-valid
    /// context: a `--session` that cannot be stored, or both context flags
    /// at once.
    pub fn new(args: &Cli) -> Result<Self> {
        let env = std::env::var_os("FF_SESSION").map(|raw| raw.to_string_lossy().into_owned());
        // `ff -v` is the version verb spelled as a flag, so it settles as that
        // verb and not as the map.
        let synthesized = (args.version && args.command.is_none()).then_some(Command::Version);
        let command = if synthesized.is_some() {
            &synthesized
        } else {
            &args.command
        };
        Self::resolve(args.json, args.session.as_deref(), env.as_deref(), command)
    }

    /// All of `new` except reading the process environment, so the
    /// flag-then-`FF_SESSION` precedence can be tested without one.
    fn resolve(
        json: bool,
        session: Option<&str>,
        env: Option<&str>,
        command: &Option<Command>,
    ) -> Result<Self> {
        // Bare `ff` is the map, and its envelope says so.
        let (name, json_capable) = match command {
            None => ("map", true),
            Some(cmd) => (cmd.name(), cmd.json_capable()),
        };
        // clap's `conflicts_with` already refuses the pair on every verb that
        // declares them; this is the same refusal stated where the meaning
        // lives, so a flattened group added without it cannot leak through.
        let past = command.as_ref().and_then(Command::past);
        let at = match (
            past.and_then(|p| p.at_op.as_deref()),
            past.and_then(|p| p.at.as_deref()),
        ) {
            (Some(_), Some(_)) => {
                return Err(Error::coded(
                    "usage/bad-flags",
                    "--at-op takes an operation id and --at takes a time: they are one \
                     reach with two doors, so naming both says nothing more than either",
                    // After the verb, not before it: the pair is declared per
                    // verb (the `Past` group), so bare `ff --at-op …` is an
                    // unknown flag before this refusal is ever reached.
                    vec![
                        format!("ff {name} --at-op <op>"),
                        format!("ff {name} --at 2h"),
                    ],
                ));
            }
            (Some(op), None) => Some(At::Op(op.to_string())),
            (None, Some(time)) => Some(At::Time(time.to_string())),
            (None, None) => None,
        };
        Ok(Ctx {
            json: json && json_capable,
            session: crate::session::resolve(session, env)?,
            command: name,
            at,
        })
    }

    /// The refusal a verb owes when it declares the context flags but cannot
    /// yet honor them. Threading an operation's ref table through every
    /// `repo.head()` and `repo.references()` site is what these verbs are
    /// waiting on; until then, saying so beats an unknown-argument error that
    /// would teach the flags do not exist.
    pub fn refuse_past(&self, verb: &str) -> Result<()> {
        let Some(at) = &self.at else { return Ok(()) };
        let flag = match at {
            At::Op(_) => "--at-op",
            At::Time(_) => "--at",
        };
        Err(Error::coded(
            "usage/at-op-unsupported",
            format!(
                "{verb} does not read a past state yet, so {flag} has nothing to place it \
                 against; the verbs that resolve a target rather than render a view take it today"
            ),
            vec![
                "ff op show <op>".into(),
                "ff op log".into(),
                "ff restore <path> --at-op <op>".into(),
            ],
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(flag: Option<&str>, env: Option<&str>) -> Result<Ctx> {
        Ctx::resolve(false, flag, env, &None)
    }

    #[test]
    fn flag_wins_over_the_environment() {
        assert_eq!(
            ctx(Some("flag"), Some("env")).unwrap().session.as_deref(),
            Some("flag")
        );
    }

    #[test]
    fn environment_fills_in_when_the_flag_is_absent() {
        assert_eq!(
            ctx(None, Some("env")).unwrap().session.as_deref(),
            Some("env")
        );
        assert_eq!(ctx(None, None).unwrap().session, None);
    }

    #[test]
    fn both_sources_are_trimmed_the_same_way() {
        assert_eq!(
            ctx(Some("  flag  "), None).unwrap().session.as_deref(),
            Some("flag")
        );
        assert_eq!(
            ctx(None, Some("  env  ")).unwrap().session.as_deref(),
            Some("env")
        );
    }

    #[test]
    fn a_bad_flag_is_fatal_and_a_bad_environment_is_not() {
        let err = ctx(Some("a\nb"), None).unwrap_err();
        assert_eq!(err.id(), "usage/bad-session");
        assert_eq!(ctx(None, Some("a\nb")).unwrap().session, None);
        // The flag still wins even when the environment is the unusable one.
        assert_eq!(
            ctx(Some("flag"), Some("a\nb")).unwrap().session.as_deref(),
            Some("flag")
        );
    }

    fn status(at_op: Option<&str>, at: Option<&str>) -> Command {
        Command::Status {
            past: crate::cli::Past {
                at_op: at_op.map(str::to_string),
                at: at.map(str::to_string),
            },
        }
    }

    #[test]
    fn json_is_ignored_by_the_verbs_that_own_their_stream() {
        let json = |command| Ctx::resolve(true, None, None, &Some(command)).unwrap().json;
        assert!(json(status(None, None)));
        assert!(!json(Command::Git { args: vec![] }));
        assert!(!json(Command::Update { check: false }));
        // And nothing turns it on that did not ask.
        assert!(!Ctx::resolve(false, None, None, &None).unwrap().json);
    }

    #[test]
    fn the_bare_command_is_named_map() {
        assert_eq!(ctx(None, None).unwrap().command, "map");
        assert_eq!(
            Ctx::resolve(false, None, None, &Some(status(None, None)))
                .unwrap()
                .command,
            "status"
        );
    }

    /// `ff op log` and `ff op show` are two shapes, so they are two names on
    /// the envelope — the anti-precedent is `ff session`, which put a listing
    /// and a diffstat under one.
    #[test]
    fn the_op_family_names_the_full_path() {
        let name = |action| {
            Ctx::resolve(false, None, None, &Some(Command::Op { action }))
                .unwrap()
                .command
        };
        assert_eq!(
            name(crate::cli::OpAction::Log {
                revset: None,
                count: 25,
                revisions: None,
                captures: false,
                past: Default::default()
            }),
            "op log"
        );
        assert_eq!(
            name(crate::cli::OpAction::Revert { op: "kqzm".into() }),
            "op revert"
        );
    }

    /// One kind per flag, and one reach: naming both doors is refused here
    /// rather than ranked, before any verb has run.
    #[test]
    fn the_two_context_flags_are_one_reach() {
        let at = |op, time| Ctx::resolve(false, None, None, &Some(status(op, time)));
        assert_eq!(at(None, None).unwrap().at, None);
        assert_eq!(
            at(Some("kqzm"), None).unwrap().at,
            Some(At::Op("kqzm".into()))
        );
        assert_eq!(
            at(None, Some("2h")).unwrap().at,
            Some(At::Time("2h".into()))
        );
        assert_eq!(
            at(Some("kqzm"), Some("2h")).unwrap_err().id(),
            "usage/bad-flags"
        );
    }

    /// A verb that only adds to now never carries the flags, so the context
    /// is empty there by construction rather than by a check at the far end.
    #[test]
    fn verbs_that_only_add_to_now_carry_no_past() {
        assert!(Command::Undo.past().is_none());
        assert!(
            Command::Commit {
                message: None,
                no_verify: false,
                branch: None,
                sign: false,
                no_sign: false,
                paths: Vec::new(),
            }
            .past()
            .is_none()
        );
        assert!(status(None, None).past().is_some());
    }
}
