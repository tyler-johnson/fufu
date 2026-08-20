//! The network lane. Two shapes live here, and they are not the same shape.
//!
//! `fetch` and `push` spawn the git binary, one call at a time. The write
//! ladder in DESIGN keeps them there until native coverage earns them, and a
//! sanctioned spawn inherits git's credential helpers whole — every SSH
//! agent, every `gh auth`, every corporate helper — which is not a thing to
//! reimplement for free. This is the counterpart to `ff trim`'s `gc --auto`,
//! the other sanctioned spawn, and it differs in one way: a fetch or a push
//! that fails is not best-effort. It is reported, with a coded error.
//!
//! `clone` is the first rung up that ladder. fufu speaks the git protocol
//! itself there — gix's blocking transport over reqwest and rustls — so the
//! negotiation, the pack, and the checkout all happen inside this process.
//! What it still reaches outside for is git's *configuration and
//! authentication* surface rather than its porcelain: one `git config -l` per
//! process (without which `url.<base>.insteadOf`, `http.proxy` and
//! `credential.helper` from the installation config would all be ignored —
//! exactly the settings that make a clone work behind a corporate proxy), a
//! credential helper when a remote asks for auth, and `ssh` for an ssh URL.
//! That is the same trade the two spawns above state, made one layer lower:
//! inherit git's credential surface whole rather than reimplement it.
//!
//! So "native" here is a claim about the protocol, not about the process
//! table, and `tests/zero_spawn.rs` says the same thing in its preamble.

use ff_core::{Error, Publish, Result};

/// Run one `git` invocation, capturing stderr so a failure can be classified
/// rather than merely observed. Progress bars are the only thing capturing
/// costs — git draws none when stderr is not a terminal, and its summary
/// lines come through either way.
fn run(cwd: &std::path::Path, args: &[&str]) -> Result<Run> {
    let output = std::process::Command::new("git")
        .current_dir(cwd)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|_| {
            Error::coded(
                "sync/no-git",
                "git is not on PATH, and fufu still spawns it to fetch and push",
                vec!["ff sync --no-fetch".into()],
            )
        })?;
    Ok(Run {
        ok: output.status.success(),
        code: output.status.code().unwrap_or(0),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}

struct Run {
    ok: bool,
    /// git's exit code. 128 is "could not reach the remote"; 1 is "the remote
    /// answered and said no".
    code: i32,
    stderr: String,
}

/// `git fetch <remote>` — every branch, no refspec and no `--prune`. Sync's
/// job is the branch underfoot, and quietly deleting every stale tracking ref
/// in the repository is a repository-wide mutation nobody asked this verb for.
pub fn fetch(cwd: &std::path::Path, remote: &str) -> Result<()> {
    let run = run(cwd, &["fetch", remote])?;
    if run.ok {
        return Ok(());
    }
    Err(Error::coded(
        "sync/fetch-failed",
        format!(
            "git fetch {remote} failed: {}",
            first_useful_line(&run.stderr)
        ),
        vec![
            format!("ff git fetch {remote}"),
            "ff sync --no-fetch".into(),
        ],
    ))
}

/// `git push`, in the one of two shapes the plan calls for. A branch with no
/// upstream is created and tracked; an existing one goes under a lease whose
/// expected value is exactly the tip this run's fetch left behind.
pub fn push(cwd: &std::path::Path, local_branch: &str, plan: &Publish) -> Result<()> {
    let (remote, remote_branch, lease, spec) = match plan {
        Publish::Create {
            remote,
            remote_branch,
            ..
        } => (
            remote.clone(),
            remote_branch.clone(),
            None,
            format!("{local_branch}:{remote_branch}"),
        ),
        Publish::Push {
            remote,
            remote_branch,
            lease,
            ..
        } => (
            remote.clone(),
            remote_branch.clone(),
            Some(format!("--force-with-lease={remote_branch}:{lease}")),
            format!("{local_branch}:{remote_branch}"),
        ),
        _ => return Ok(()),
    };
    let args: Vec<&str> = match lease {
        Some(ref lease) => vec!["push", lease.as_str(), remote.as_str(), spec.as_str()],
        None => vec!["push", "-u", remote.as_str(), spec.as_str()],
    };

    let run = run(cwd, &args)?;
    if run.ok {
        return Ok(());
    }
    if run.code == 128 {
        return Err(Error::coded(
            "publish/unreachable",
            format!(
                "could not reach {remote}: {}",
                first_useful_line(&run.stderr)
            ),
            vec![format!("ff git push {remote}"), "ff status".into()],
        ));
    }
    if run.stderr.contains("stale info") {
        return Err(Error::coded(
            "publish/lease-refused",
            format!(
                "{remote}/{remote_branch} moved since you last looked, so nothing \
                 was pushed — your commits are still here, and ff sync takes in \
                 what arrived"
            ),
            vec!["ff sync".into(), "ff publish".into()],
        ));
    }
    if run.stderr.contains("[remote rejected]") {
        return Err(Error::coded(
            "publish/rejected",
            format!(
                "{remote} refused the push: {}",
                first_useful_line(&run.stderr)
            ),
            vec![format!("ff git push {remote}"), "ff status".into()],
        ));
    }
    Err(Error::coded(
        "publish/failed",
        format!(
            "git push to {remote} failed: {}",
            first_useful_line(&run.stderr)
        ),
        vec![format!("ff git push {remote}"), "ff status".into()],
    ))
}

/// `ff clone`'s whole wire call: negotiate, take the pack, check out.
///
/// The three builder settings are exactly gix's, with no shape of our own
/// layered on top — a surface that is one thing wearing another's name is
/// how a flag ends up meaning something subtly different from the flag it
/// was copied from.
///
/// `PrepareFetch` deletes the half-built directory when it drops, so a
/// Ctrl-C anywhere in here leaves nothing behind. The handler that sets the
/// flag is installed in `main`, before any verb runs.
pub fn clone(opts: Clone<'_>) -> Result<gix::Repository> {
    let mut prepare = gix::prepare_clone(opts.url, opts.dir).map_err(bad_url)?;
    if let Some(branch) = opts.branch {
        prepare = prepare
            .with_ref_name(Some(branch))
            .map_err(|_| bad_ref(branch))?;
    }
    if let Some(depth) = opts.depth {
        prepare = prepare.with_shallow(gix::remote::fetch::Shallow::DepthAtRemote(depth));
    }
    prepare = prepare.with_remote_name(opts.remote).map_err(|err| {
        Error::coded(
            "clone/bad-url",
            format!("{} is not a usable remote name: {err}", opts.remote),
            vec![],
        )
    })?;

    // gix takes the progress by value, so the line is taken back off the
    // terminal through a handle rather than through the bar itself — the
    // clone's own report has to start on a clean line either way it went.
    let progress = crate::progress::Bar::new("receiving objects", opts.progress);
    let line = progress.handle();
    let fetched = prepare.fetch_then_checkout(progress, &gix::interrupt::IS_INTERRUPTED);
    line.clear();
    let (mut checkout, _outcome) = fetched.map_err(|err| classify(opts.url, &err))?;

    let progress = crate::progress::Bar::new("checking out files", opts.progress);
    let line = progress.handle();
    let checked_out = checkout.main_worktree(progress, &gix::interrupt::IS_INTERRUPTED);
    line.clear();
    let (repo, _outcome) = checked_out.map_err(|err| {
        Error::coded(
            "clone/failed",
            format!("the pack arrived and the working tree could not be written: {err}"),
            vec![],
        )
    })?;
    Ok(repo)
}

/// What `ff clone` was asked for. A struct rather than six positional
/// parameters, because four of them are `Option`s of two types.
pub struct Clone<'a> {
    pub url: &'a str,
    pub dir: &'a std::path::Path,
    pub branch: Option<&'a str>,
    pub depth: Option<std::num::NonZeroU32>,
    pub remote: &'a str,
    /// Draw the progress line. Off for `--json`, and off again inside the
    /// renderer when stderr is not a terminal.
    pub progress: bool,
}

fn bad_url(err: gix::clone::Error) -> Error {
    Error::coded(
        "clone/bad-url",
        format!("that is not a repository fufu can address: {err}"),
        vec![],
    )
}

fn bad_ref(branch: &str) -> Error {
    Error::coded(
        "clone/bad-url",
        format!("{branch} is not a branch name git would accept"),
        vec![],
    )
}

/// Which of the three ways a clone fails this was. The split is the one
/// `fetch` and `push` already draw: could not reach it, versus reached it and
/// was told no.
fn classify(url: &str, err: &gix::clone::fetch::Error) -> Error {
    use gix::clone::fetch::Error as Fetch;
    match err {
        Fetch::Connect(_) => Error::coded(
            "clone/unreachable",
            format!("could not reach {url}: {}", first_useful_line(&chain(err))),
            vec![format!("ff git ls-remote {url}")],
        ),
        Fetch::RefNameMissing { .. } | Fetch::RefNameAmbiguous { .. } => Error::coded(
            "clone/refused",
            format!("{url} answered, and {}", first_useful_line(&chain(err))),
            vec![format!("ff git ls-remote {url}")],
        ),
        // Everything the remote itself declined — auth, a repository that is
        // not there, a protocol the far side would not speak.
        _ => Error::coded(
            "clone/refused",
            format!(
                "{url} refused the clone: {}",
                first_useful_line(&chain(err))
            ),
            vec![format!("ff git ls-remote {url}"), "ff doctor".into()],
        ),
    }
}

/// A `std::error::Error` chain flattened into the transcript shape
/// [`first_useful_line`] already knows how to read.
fn chain(err: &dyn std::error::Error) -> String {
    let mut out = err.to_string();
    let mut source = err.source();
    while let Some(err) = source {
        out.push('\n');
        out.push_str(&err.to_string());
        source = err.source();
    }
    out
}

/// The one line of git's stderr worth putting in an error: the first
/// `fatal:`, `error:` or ` ! ` line, or failing all three the last non-empty
/// line. git's stderr is a transcript and a coded error is one sentence.
fn first_useful_line(stderr: &str) -> String {
    let lines: Vec<&str> = stderr.lines().collect();
    for line in &lines {
        if line.contains("fatal:") || line.contains("error:") || line.contains(" ! ") {
            return line.to_string();
        }
    }
    lines
        .iter()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.to_string())
        .unwrap_or_else(|| "git said nothing".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// A git call that must succeed; panics with stderr if it does not.
    fn git(dir: &std::path::Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .current_dir(dir)
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A bare remote plus a clone holding one commit on main, tracking it.
    /// The `TempDir` lives in the return so the paths outlive the calls.
    fn fixture() -> (TempDir, PathBuf, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let remote = tmp.path().join("remote.git");
        let remote_s = remote.to_string_lossy().into_owned();
        git(tmp.path(), &["init", "--bare", &remote_s]);
        let clone = tmp.path().join("clone");
        let clone_s = clone.to_string_lossy().into_owned();
        git(tmp.path(), &["clone", &remote_s, &clone_s]);
        std::fs::write(clone.join("a.txt"), "hello\n").unwrap();
        git(&clone, &["add", "a.txt"]);
        git(&clone, &["commit", "-m", "one"]);
        git(&clone, &["push", "-u", "origin", "main"]);
        (tmp, remote, clone)
    }

    #[test]
    fn first_useful_line_picks_the_fatal() {
        assert_eq!(
            first_useful_line("fatal: '../nope.git' does not appear to be a git repository"),
            "fatal: '../nope.git' does not appear to be a git repository"
        );
        assert_eq!(
            first_useful_line(
                " ! [rejected]        main -> main (stale info)\nerror: failed to push some refs to '…'"
            ),
            " ! [rejected]        main -> main (stale info)"
        );
        assert_eq!(first_useful_line(""), "git said nothing");
    }

    #[test]
    fn a_lease_that_holds_pushes_and_moves_the_tracking_ref() {
        let (_tmp, _remote, clone) = fixture();
        let lease = git(&clone, &["rev-parse", "refs/remotes/origin/main"]);
        git(&clone, &["commit", "--amend", "-m", "two"]);
        let tip = git(&clone, &["rev-parse", "HEAD"]);
        let res = push(
            &clone,
            "main",
            &Publish::Push {
                remote: "origin".into(),
                remote_branch: "main".into(),
                lease,
                tip: tip.clone(),
                shape: ff_core::PushShape::Replace,
            },
        );
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(git(&clone, &["rev-parse", "refs/remotes/origin/main"]), tip);
    }

    #[test]
    fn a_stale_lease_is_refused_and_moves_nothing() {
        let (tmp, remote, clone1) = fixture();
        let clone2 = tmp.path().join("clone2");
        let clone2_s = clone2.to_string_lossy().into_owned();
        let remote_s = remote.to_string_lossy().into_owned();
        git(tmp.path(), &["clone", &remote_s, &clone2_s]);
        std::fs::write(clone2.join("b.txt"), "b\n").unwrap();
        git(&clone2, &["add", "b.txt"]);
        git(&clone2, &["commit", "-m", "two"]);
        git(&clone2, &["push", "origin", "main"]);
        let stale = git(&clone1, &["rev-parse", "refs/remotes/origin/main"]);
        let res = push(
            &clone1,
            "main",
            &Publish::Push {
                remote: "origin".into(),
                remote_branch: "main".into(),
                lease: stale.clone(),
                tip: git(&clone1, &["rev-parse", "HEAD"]),
                shape: ff_core::PushShape::Replace,
            },
        );
        let err = res.unwrap_err();
        assert_eq!(err.id(), "publish/lease-refused");
        assert_eq!(
            git(&clone1, &["rev-parse", "refs/remotes/origin/main"]),
            stale
        );
    }

    #[test]
    fn a_dead_remote_is_unreachable() {
        let (tmp, _remote, clone) = fixture();
        let dead = tmp.path().join("nope.git");
        let dead_s = dead.to_string_lossy().into_owned();
        git(&clone, &["remote", "set-url", "origin", &dead_s]);
        let res = push(
            &clone,
            "main",
            &Publish::Push {
                remote: "origin".into(),
                remote_branch: "main".into(),
                lease: git(&clone, &["rev-parse", "refs/remotes/origin/main"]),
                tip: git(&clone, &["rev-parse", "HEAD"]),
                shape: ff_core::PushShape::Replace,
            },
        );
        assert_eq!(res.unwrap_err().id(), "publish/unreachable");
        let res = fetch(&clone, "origin");
        assert_eq!(res.unwrap_err().id(), "sync/fetch-failed");
    }

    #[test]
    fn a_new_branch_is_created_and_tracked() {
        let (_tmp, remote, clone) = fixture();
        git(&clone, &["checkout", "-b", "feature"]);
        let tip = git(&clone, &["rev-parse", "HEAD"]);
        let res = push(
            &clone,
            "feature",
            &Publish::Create {
                remote: "origin".into(),
                remote_branch: "feature".into(),
                tip: tip.clone(),
            },
        );
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(git(&remote, &["rev-parse", "refs/heads/feature"]), tip);
        assert_eq!(
            git(&clone, &["config", "--get", "branch.feature.merge"]),
            "refs/heads/feature"
        );
    }

    #[test]
    fn the_quiet_variants_spawn_nothing() {
        let (tmp, _remote, clone) = fixture();
        let dead = tmp.path().join("nope.git");
        let dead_s = dead.to_string_lossy().into_owned();
        git(&clone, &["remote", "set-url", "origin", &dead_s]);
        for plan in [Publish::NoRemote, Publish::Blocked, Publish::UpToDate] {
            assert!(push(&clone, "main", &plan).is_ok());
        }
    }
}
