mod autotrim;
mod cadence;
mod capture;
mod cli;
mod cmd;
mod ctx;
mod explain;
mod help;
mod machine;
mod pager;
mod provenance;
mod render;
mod session;

mod selfupdate;

use clap::Parser;

/// The pre-dispatch gate: refuse the one flag combination clap used to
/// refuse for us, then build the context the verbs run against.
///
/// `args_conflicts_with_subcommands` did the refusing until now, but it bans
/// every root-level argument beside a subcommand — including the ones
/// declared `global = true`, which exist precisely to ride any verb. That
/// made `ff --json status` a usage error, and would have made `ff --at-op
/// <op> status` one too. Bare-snap's `-m` was the only real conflict it ever
/// caught, so it is named here instead and the globals go free.
fn settle(args: &cli::Cli) -> ff_core::Result<ctx::Ctx> {
    if args.message.is_some() && args.command.is_some() {
        return Err(ff_core::Error::coded(
            "usage/bad-flags",
            "-m is bare ff's snapshot message; it does not ride another verb",
            vec!["ff -m <msg>".into(), "ff <verb> -m <msg>".into()],
        ));
    }
    ctx::Ctx::new(args)
}

/// Render one failure and exit. Both failure paths come through here — the
/// usage error raised before there is a `Ctx`, and anything a verb returns —
/// so which rendering a caller gets depends on the flag they passed and never
/// on how early the failure happened.
fn report(json: bool, command: &str, err: &ff_core::Error) -> ! {
    if json {
        // The envelope shape lives in one place; a failure to emit it
        // must not mask the failure being reported.
        let _ = machine::emit_error(command, err);
    } else {
        // Human rendering on stderr
        eprintln!("ff: {err}");
        if !err.exits().is_empty() {
            eprintln!("  try:");
            for hint in err.exits() {
                eprintln!("    {hint}");
            }
        }
    }
    std::process::exit(err.exit_code())
}

fn main() {
    // A Ctrl-C between ref lock and ref commit would leave a stale `.lock`
    // behind and silently turn every later capture into Contended. gix's
    // interrupt handler cleans up tempfiles (which back ref locks) on
    // signals; the engine itself never installs handlers.
    let _interrupt = unsafe { ff_core::gix::interrupt::init_handler(1, || {}) };

    let args = cli::Cli::parse();

    // Whatever must be settled before dispatch: the one flag combination
    // clap no longer refuses, and the invocation context every verb reads.
    // The envelope says "snap" because a command line this broken has no
    // verb to name — which is what it said before, too.
    let ctx = match settle(&args) {
        Ok(ctx) => ctx,
        // No verb survived parsing, so the raw flag decides the rendering.
        Err(err) => report(args.json, "snap", &err),
    };

    let result = match args.command {
        None => cmd::snap::run(&ctx, args.message),
        Some(cli::Command::Status) => cmd::status::run(&ctx),
        Some(cli::Command::Log {
            count,
            revisions,
            commits,
            ops,
            session,
        }) => cmd::log::run(&ctx, count, revisions, commits, ops, session),
        Some(cli::Command::Evolog { count, session }) => cmd::evolog::run(&ctx, count, session),
        Some(cli::Command::Git { args: git_args }) => cmd::git::run(&ctx, git_args),
        Some(cli::Command::Restore { at, all, paths }) => cmd::restore::run(&ctx, at, all, paths),
        Some(cli::Command::Trim { dry_run, gone }) => cmd::trim::run(&ctx, dry_run, gone),
        Some(cli::Command::Commit {
            message,
            no_verify,
            branch,
        }) => cmd::commit::run(&ctx, message, no_verify, branch),
        Some(cli::Command::Switch { target }) => cmd::switch::run(&ctx, target),
        Some(cli::Command::Undo { op, force }) => cmd::undo::run(&ctx, op, force),
        Some(cli::Command::Branch { name, delete }) => cmd::branch::run(&ctx, name, delete),
        Some(cli::Command::Start {
            target,
            message,
            branch,
        }) => cmd::start::run(&ctx, target, message, branch),
        Some(cli::Command::Describe { message, branch }) => {
            cmd::describe::run(&ctx, message, branch)
        }
        Some(cli::Command::Hook { kind }) => cmd::hook::run(&ctx, kind),
        Some(cli::Command::Config {
            key,
            value,
            unset,
            global,
        }) => cmd::config::run(&ctx, key, value, unset, global),
        Some(cli::Command::Doctor { fix }) => cmd::doctor::run(&ctx, fix),
        Some(cli::Command::Explain { id, list }) => cmd::explain::run(&ctx, id, list),
        Some(cli::Command::Update { check }) => cmd::update::run(&ctx, check),
        Some(cli::Command::Session { action }) => {
            let (action_str, name) = match action {
                Some(cli::SessionAction::List) => (Some("list"), None),
                Some(cli::SessionAction::Diff { name }) => (Some("diff"), name),
                None => (None, None),
            };
            cmd::session::run(&ctx, action_str, name)
        }
    };

    if let Err(err) = result {
        report(ctx.json, ctx.command, &err);
    }

    // Exit without unwinding: skips munmap/thread teardown of the repo, which
    // is pure latency. Stdout is flushed explicitly first.
    use std::io::Write;
    let _ = std::io::stdout().flush();
    std::process::exit(0);
}
