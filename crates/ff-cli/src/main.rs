mod autotrim;
mod cadence;
mod capture;
mod cli;
mod cmd;
mod explain;
mod help;
mod machine;
mod pager;
mod provenance;
mod render;
mod session;

mod selfupdate;

use clap::Parser;

fn main() {
    // A Ctrl-C between ref lock and ref commit would leave a stale `.lock`
    // behind and silently turn every later capture into Contended. gix's
    // interrupt handler cleans up tempfiles (which back ref locks) on
    // signals; the engine itself never installs handlers.
    let _interrupt = unsafe { ff_core::gix::interrupt::init_handler(1, || {}) };

    let args = cli::Cli::parse();

    // Validate --session early: an explicit flag is a hard error.
    if let Some(ref sess) = args.session
        && let Err(e) = crate::session::parse(sess)
    {
        let _ = machine::emit_error("snap", &e);
        std::process::exit(e.exit_code());
    }

    // Record the session override so provenance constructors see it.
    crate::session::set_override(args.session.map(|s| crate::session::parse(&s).unwrap()));

    // Bookkeeping for error rendering: whether --json was passed and the
    // canonical command name. Both are captured here before the dispatch
    // so the error handler below has them without threading anything.
    let (is_json, cmd_name) = match &args.command {
        None => (args.json, "snap"),
        Some(cli::Command::Status) => (args.json, "status"),
        Some(cli::Command::Log { .. }) => (args.json, "log"),
        Some(cli::Command::Evolog { .. }) => (args.json, "evolog"),
        Some(cli::Command::Git { .. }) => (false, "git"),
        Some(cli::Command::Restore { .. }) => (args.json, "restore"),
        Some(cli::Command::Trim { .. }) => (args.json, "trim"),
        Some(cli::Command::Commit { .. }) => (args.json, "commit"),
        Some(cli::Command::Switch { .. }) => (args.json, "switch"),
        Some(cli::Command::Undo { .. }) => (args.json, "undo"),
        Some(cli::Command::Branch { .. }) => (args.json, "branch"),
        Some(cli::Command::Start { .. }) => (args.json, "start"),
        Some(cli::Command::Describe { .. }) => (args.json, "describe"),
        Some(cli::Command::Hook { .. }) => (false, "hook"),
        Some(cli::Command::Config { .. }) => (args.json, "config"),
        Some(cli::Command::Doctor { .. }) => (args.json, "doctor"),
        Some(cli::Command::Explain { .. }) => (args.json, "explain"),
        Some(cli::Command::Update { .. }) => (false, "update"),
        Some(cli::Command::Session { .. }) => (args.json, "session"),
    };

    let result = match args.command {
        None => cmd::snap::run(args.message, args.json),
        Some(cli::Command::Status) => cmd::status::run(args.json),
        Some(cli::Command::Log {
            count,
            commits,
            ops,
            session,
        }) => cmd::log::run(args.json, count, commits, ops, session),
        Some(cli::Command::Evolog { count, session }) => {
            cmd::evolog::run(args.json, count, session)
        }
        Some(cli::Command::Git { args: git_args }) => cmd::git::run(git_args),
        Some(cli::Command::Restore { at, all, paths }) => {
            cmd::restore::run(at, all, paths, args.json)
        }
        Some(cli::Command::Trim { dry_run, gone }) => cmd::trim::run(dry_run, gone, args.json),
        Some(cli::Command::Commit {
            message,
            no_verify,
            branch,
        }) => cmd::commit::run(message, no_verify, branch, args.json),
        Some(cli::Command::Switch { target }) => cmd::switch::run(target, args.json),
        Some(cli::Command::Undo { op, force }) => cmd::undo::run(op, force, args.json),
        Some(cli::Command::Branch { name, delete }) => cmd::branch::run(name, delete, args.json),
        Some(cli::Command::Start {
            target,
            message,
            branch,
        }) => cmd::start::run(target, message, branch, args.json),
        Some(cli::Command::Describe { message, branch }) => {
            cmd::describe::run(message, branch, args.json)
        }
        Some(cli::Command::Hook { kind }) => cmd::hook::run(kind),
        Some(cli::Command::Config {
            key,
            value,
            unset,
            global,
        }) => cmd::config::run(key, value, unset, global, args.json),
        Some(cli::Command::Doctor { fix }) => cmd::doctor::run(fix, args.json),
        Some(cli::Command::Explain { id, list }) => cmd::explain::run(id, list, args.json),
        Some(cli::Command::Update { check }) => cmd::update::run(check),
        Some(cli::Command::Session { action }) => {
            let (action_str, name) = match action {
                Some(cli::SessionAction::List) => (Some("list"), None),
                Some(cli::SessionAction::Diff { name }) => (Some("diff"), name),
                None => (None, None),
            };
            cmd::session::run(action_str, name, args.json)
        }
    };

    if let Err(err) = result {
        if is_json {
            // The envelope shape lives in one place; a failure to emit it
            // must not mask the failure being reported.
            let _ = machine::emit_error(cmd_name, &err);
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
        std::process::exit(err.exit_code());
    }

    // Exit without unwinding: skips munmap/thread teardown of the repo, which
    // is pure latency. Stdout is flushed explicitly first.
    use std::io::Write;
    let _ = std::io::stdout().flush();
    std::process::exit(0);
}
