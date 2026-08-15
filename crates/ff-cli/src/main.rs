mod autotrim;
mod cadence;
mod capture;
mod cli;
mod cmd;
mod help;
mod machine;
mod pager;
mod provenance;
mod render;

mod selfupdate;

use clap::Parser;

fn main() {
    // A Ctrl-C between ref lock and ref commit would leave a stale `.lock`
    // behind and silently turn every later capture into Contended. gix's
    // interrupt handler cleans up tempfiles (which back ref locks) on
    // signals; the engine itself never installs handlers.
    let _interrupt = unsafe { ff_core::gix::interrupt::init_handler(1, || {}) };

    let args = cli::Cli::parse();

    // Bookkeeping for error rendering: whether --json was passed and the
    // canonical command name. Both are captured here before the dispatch
    // so the error handler below has them without threading anything.
    let (is_json, cmd_name) = match &args.command {
        None => (args.json, "snap"),
        Some(cli::Command::Status { json }) => (*json, "status"),
        Some(cli::Command::Log { json, .. }) => (*json, "log"),
        Some(cli::Command::Evolog { json, .. }) => (*json, "evolog"),
        Some(cli::Command::Git { .. }) => (false, "git"),
        Some(cli::Command::Restore { json, .. }) => (*json, "restore"),
        Some(cli::Command::Trim { json, .. }) => (*json, "trim"),
        Some(cli::Command::Commit { json, .. }) => (*json, "commit"),
        Some(cli::Command::Switch { json, .. }) => (*json, "switch"),
        Some(cli::Command::Undo { json, .. }) => (*json, "undo"),
        Some(cli::Command::Branch { json, .. }) => (*json, "branch"),
        Some(cli::Command::Start { json, .. }) => (*json, "start"),
        Some(cli::Command::Describe { json, .. }) => (*json, "describe"),
        Some(cli::Command::Hook { .. }) => (false, "hook"),
        Some(cli::Command::Config { json, .. }) => (*json, "config"),
        Some(cli::Command::Doctor { json, .. }) => (*json, "doctor"),
        Some(cli::Command::Update { .. }) => (false, "update"),
    };

    let result = match args.command {
        None => cmd::snap::run(args.message, args.json),
        Some(cli::Command::Status { json }) => cmd::status::run(json),
        Some(cli::Command::Log {
            json,
            count,
            commits,
            ops,
        }) => cmd::log::run(json, count, commits, ops),
        Some(cli::Command::Evolog { json, count }) => cmd::evolog::run(json, count),
        Some(cli::Command::Git { args }) => cmd::git::run(args),
        Some(cli::Command::Restore {
            at,
            all,
            paths,
            json,
        }) => cmd::restore::run(at, all, paths, json),
        Some(cli::Command::Trim {
            dry_run,
            gone,
            json,
        }) => cmd::trim::run(dry_run, gone, json),
        Some(cli::Command::Commit {
            message,
            no_verify,
            branch,
            json,
        }) => cmd::commit::run(message, no_verify, branch, json),
        Some(cli::Command::Switch { target, json }) => cmd::switch::run(target, json),
        Some(cli::Command::Undo { op, force, json }) => cmd::undo::run(op, force, json),
        Some(cli::Command::Branch { name, delete, json }) => cmd::branch::run(name, delete, json),
        Some(cli::Command::Start {
            target,
            message,
            branch,
            json,
        }) => cmd::start::run(target, message, branch, json),
        Some(cli::Command::Describe {
            message,
            branch,
            json,
        }) => cmd::describe::run(message, branch, json),
        Some(cli::Command::Hook { kind }) => cmd::hook::run(kind),
        Some(cli::Command::Config {
            key,
            value,
            unset,
            global,
            json,
        }) => cmd::config::run(key, value, unset, global, json),
        Some(cli::Command::Doctor { fix, json }) => cmd::doctor::run(fix, json),
        Some(cli::Command::Update { check }) => cmd::update::run(check),
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
