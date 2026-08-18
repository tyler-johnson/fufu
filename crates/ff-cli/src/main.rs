mod autotrim;
mod cadence;
mod capture;
mod cli;
mod cmd;
mod ctx;
mod explain;
mod graph;
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
/// <op> status` one too. The map's retired `-m` and branch scope were the
/// only real conflicts it ever caught, so they are named here instead and
/// the globals go free.
fn settle(args: &cli::Cli) -> ff_core::Result<ctx::Ctx> {
    // `-m` is the retired snapshot message: a removal, refused with or
    // without a subcommand, and answered with what bare ff is now.
    if args.message.is_some() {
        return Err(ff_core::Error::coded(
            "usage/bad-flags",
            "-m is gone: bare ff is the map now, and capture is automatic — every \
             verb captures first, so there is no snapshot to name",
            vec![
                "ff".into(),
                "ff describe -m <msg>".into(),
                "ff commit -m <msg>".into(),
            ],
        ));
    }
    // `-n` and `--all` are the map's branch scope; beside a subcommand they
    // mean nothing, so they are refused rather than silently ignored.
    if args.command.is_some() && (args.branches.is_some() || args.all) {
        return Err(ff_core::Error::coded(
            "usage/bad-flags",
            "-n and --all are bare ff's branch scope; they do not ride another verb",
            vec!["ff -n 5".into(), "ff map -n 5".into(), "ff log -n 5".into()],
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

    // Whatever must be settled before dispatch: the flag combinations clap
    // no longer refuses, and the invocation context every verb reads.
    // The envelope says "map" because a command line this broken has no
    // verb to name, and bare ff is the map.
    let ctx = match settle(&args) {
        Ok(ctx) => ctx,
        // No verb survived parsing, so the raw flag decides the rendering.
        Err(err) => report(args.json, "map", &err),
    };

    let result = match args.command {
        None => cmd::map::run(&ctx, args.branches, args.all),
        // Spelled out, the map takes its scope after its own name — which is
        // also why the root flags refuse to ride a verb.
        Some(cli::Command::Map { branches, all }) => cmd::map::run(&ctx, branches, all),
        Some(cli::Command::Status { .. }) => cmd::status::run(&ctx),
        Some(cli::Command::Log {
            count,
            revisions,
            commits,
            ops,
            ..
        }) => cmd::log::run(&ctx, count, revisions, commits, ops),
        Some(cli::Command::Evolog { count, .. }) => cmd::evolog::run(&ctx, count),
        Some(cli::Command::Git { args: git_args }) => cmd::git::run(&ctx, git_args),
        Some(cli::Command::Restore {
            from, all, paths, ..
        }) => cmd::restore::run(&ctx, from, all, paths),
        Some(cli::Command::Trim { dry_run, gone }) => cmd::trim::run(&ctx, dry_run, gone),
        Some(cli::Command::Commit {
            message,
            no_verify,
            branch,
        }) => cmd::commit::run(&ctx, message, no_verify, branch),
        Some(cli::Command::Switch { target }) => cmd::switch::run(&ctx, target),
        Some(cli::Command::Undo) => cmd::undo::run(&ctx),
        Some(cli::Command::Redo) => cmd::undo::redo(&ctx),
        Some(cli::Command::Op { action }) => cmd::op::run(&ctx, action),
        Some(cli::Command::Branch { action, .. }) => cmd::branch::run(&ctx, action),
        Some(cli::Command::Start {
            target,
            message,
            branch,
        }) => cmd::start::run(&ctx, target, message, branch),
        Some(cli::Command::Describe {
            rev,
            message,
            branch,
        }) => cmd::describe::run(&ctx, rev, message, branch),
        Some(cli::Command::Absorb { into, paths }) => cmd::absorb::run(&ctx, into, paths),
        Some(cli::Command::Lift { from, paths }) => cmd::lift::run(&ctx, from, paths),
        Some(cli::Command::Restack { branch, onto }) => cmd::restack::run(&ctx, branch, onto),
        Some(cli::Command::Edit { rev }) => cmd::edit::run(&ctx, rev),
        Some(cli::Command::Done { abandon }) => cmd::done::run(&ctx, abandon),
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
        // The foreign verbs answer and stop; none of them reaches a repository.
        Some(cli::Command::Checkout { args }) => cmd::foreign::checkout(&args),
        Some(cli::Command::Diff { args }) => cmd::foreign::diff(&args),
        Some(cli::Command::Stash { args }) => cmd::foreign::stash(&args),
        Some(cli::Command::Pull { args }) => cmd::foreign::pull(&args),
        Some(cli::Command::Rebase { args }) => cmd::foreign::rebase(&args),
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
