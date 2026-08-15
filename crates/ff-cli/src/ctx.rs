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

use ff_core::Result;

use crate::cli::{Cli, Command};

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
}

impl Ctx {
    /// Settle the invocation state from the parsed command line. The only
    /// failure is a `--session` that cannot be stored; it lands before
    /// dispatch, so no verb ever runs against a half-valid context.
    pub fn new(args: &Cli) -> Result<Self> {
        let env = std::env::var_os("FF_SESSION").map(|raw| raw.to_string_lossy().into_owned());
        Self::resolve(
            args.json,
            args.session.as_deref(),
            env.as_deref(),
            &args.command,
        )
    }

    /// All of `new` except reading the process environment, so the
    /// flag-then-`FF_SESSION` precedence can be tested without one.
    fn resolve(
        json: bool,
        session: Option<&str>,
        env: Option<&str>,
        command: &Option<Command>,
    ) -> Result<Self> {
        // Bare `ff` is the snapshot verb, and its envelope says so.
        let (command, json_capable) = match command {
            None => ("snap", true),
            Some(cmd) => (cmd.name(), cmd.json_capable()),
        };
        Ok(Ctx {
            json: json && json_capable,
            session: crate::session::resolve(session, env)?,
            command,
        })
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

    #[test]
    fn json_is_ignored_by_the_verbs_that_own_their_stream() {
        let json = |command| Ctx::resolve(true, None, None, &Some(command)).unwrap().json;
        assert!(json(Command::Status));
        assert!(!json(Command::Git { args: vec![] }));
        assert!(!json(Command::Update { check: false }));
        // And nothing turns it on that did not ask.
        assert!(!Ctx::resolve(false, None, None, &None).unwrap().json);
    }

    #[test]
    fn the_bare_command_is_named_snap() {
        assert_eq!(ctx(None, None).unwrap().command, "snap");
        assert_eq!(
            Ctx::resolve(false, None, None, &Some(Command::Status))
                .unwrap()
                .command,
            "status"
        );
    }
}
