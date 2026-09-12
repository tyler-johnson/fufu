//! The network lane. Two shapes live here, and they are not the same shape.
//!
//! `fetch` and `clone` speak the git protocol themselves — gix's blocking
//! transport over reqwest and rustls — so the negotiation and the pack happen
//! inside this process, and clone's checkout with them. What they still reach
//! outside for is git's *configuration and authentication* surface rather
//! than its porcelain: one `git config -l` per process (without which
//! `url.<base>.insteadOf` and `credential.helper` from the installation
//! config would be ignored — the settings that make a fetch work at all
//! behind a credential helper), a credential helper when a remote asks for
//! auth, and `ssh` for an ssh URL. Inherit git's credential surface whole
//! rather than reimplement it. What this backend does not honor is
//! `http.proxy`: reqwest here is built without proxy support, and the
//! connect probe below is faithful to that.
//!
//! `fetch` has two callers with two temperaments, and [`FetchOptions`] is
//! the difference between them. `ff pull` fetches in the foreground, with
//! no deadline and every prompt allowed. The fetch lane fetches behind `ff
//! status`, where a hang is the verb's answer arriving late and a prompt is
//! a question nobody asked: it runs under a deadline and with fufu's own
//! prompt, ssh's, and git's terminal prompt all off. Both prune.
//!
//! `push` still spawns the git binary, one call at a time, and not because
//! the ladder has not reached it: gix ships the fetch half of the protocol
//! and nothing that sends a pack, so there is no native push to climb to.
//! That is a fact about the dependency, worth writing down so nobody
//! rediscovers it. The spawn is the counterpart to `ff trim`'s `gc --auto`,
//! the other sanctioned one, and differs in a way that matters: a push that
//! fails is not best-effort. It is reported, with a coded error.
//!
//! One more spawn exists, and only in a repository that is broken in a
//! particular way: a linked worktree's admin dir holding a `gitdir` file
//! without a readable `commondir`, which is the state such a directory
//! passes through while it is being created or removed. gix's fetch opens
//! every one of them and fails the whole fetch on the first it cannot,
//! where git's own worktree walk skips it and fetches anyway. So the
//! budget is three cases: native fetch, git's `push`, and git's `fetch`
//! when the repository is in a state gix's fetch refuses and git's ignores.
//!
//! So "native" here is a claim about the protocol, not about the process
//! table, and `tests/zero_spawn.rs` says the same thing in its preamble.

use ff_core::{Error, Push, Result};

/// Run one `git` invocation — `push`, and [`fetch`]'s fallback for a
/// repository gix will not fetch in — capturing stderr
/// so a failure can be classified rather than merely observed. Progress bars
/// are the only thing capturing costs: git draws none when stderr is not a
/// terminal, and its summary lines come through either way.
fn run(cwd: &std::path::Path, args: &[&str]) -> Result<Run> {
    run_env(cwd, args, &[])
}

/// [`run`] with environment for the child: the fallback fetch's
/// `GIT_TERMINAL_PROMPT=0` when the caller may not prompt.
fn run_env(cwd: &std::path::Path, args: &[&str], env: &[(&str, &str)]) -> Result<Run> {
    let output = std::process::Command::new("git")
        .current_dir(cwd)
        .args(args)
        .envs(env.iter().copied())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|_| {
            Error::coded(
                "push/no-git",
                "git is not on PATH, and fufu still spawns it to push — \
                 fetching no longer needs it",
                vec!["ff git push".into()],
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

/// The repository as the wire needs it: git's own configuration and
/// authentication surface switched on.
///
/// Every other verb opens through `ff_core::discover`, which leaves
/// `git_binary` off and so never reaches outside this process — that is what
/// `tests/zero_spawn.rs` proves, and it must keep being true. The wire is the
/// one place where ignoring git's installation config would be wrong rather
/// than merely different: `url.<base>.insteadOf` and `credential.helper`
/// live there, and a fetch that skipped them would fail exactly where a
/// credential helper is the thing making it work. So this handle costs one
/// `git config -l` per process, which is the trade `ff clone` already makes
/// one function down.
///
/// `overrides` are config lines laid over everything read, the way `git -c`
/// lays them: the lane's `gitoxide.credentials.terminalPrompt=false`, which
/// is `GIT_TERMINAL_PROMPT=0` spelled for gix. Empty for a caller that may
/// prompt.
fn wire_repo(cwd: &std::path::Path, overrides: &[&str]) -> Result<gix::Repository> {
    let mut options = gix::open::Options::default();
    options.permissions.config.git_binary = true;
    if !overrides.is_empty() {
        options =
            options.config_overrides(overrides.iter().map(|line| gix::bstr::BString::from(*line)));
    }
    gix::open_opts(cwd, options).map_err(|err| {
        Error::coded(
            "pull/fetch-failed",
            format!("could not open the repository to fetch from: {err}"),
            vec!["ff doctor".into()],
        )
    })
}

/// How one fetch behaves: the difference between `ff pull`'s foreground
/// fetch and the lane's fetch behind a reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FetchOptions {
    /// Give up after this long. `None` is the foreground: a fetch the person
    /// asked for waits as long as the remote takes.
    ///
    /// What the bound covers, by transport: an http connect, probed before
    /// the transport is built; every http request, as reqwest's per-request
    /// deadline (headers within it, and no single body read stalling longer
    /// — a healthy long pack is not cut); and the negotiation and pack read
    /// on every transport, through gix's interrupt flag. An ssh connect is
    /// bounded by ssh's own `ConnectTimeout`. What it does not cover is a
    /// handshake that stalls after connecting on ssh or a file remote, which
    /// gix checks no flag during — the same hole git has without
    /// `ServerAliveInterval`.
    pub deadline: Option<std::time::Duration>,
    /// May the fetch ask a person for anything? `false` turns off fufu's own
    /// credential prompt, ssh's (`BatchMode=yes`), and git's terminal prompt
    /// in the fallback. A configured credential helper still runs, and a GUI
    /// helper may still ask on its own — that is the helper's, not the
    /// terminal's.
    pub interactive: bool,
    /// Delete the tracking refs of copies the remote no longer holds.
    pub prune: bool,
    /// Fetch tags too. `ff pull` does, as git does; the lane does not — a
    /// tag write lands under `TRACKED_PREFIXES`, and the next verb would
    /// report it as motion made outside fufu.
    pub tags: bool,
}

/// Fetch from `remote` — every branch its configured refspecs name, and
/// under `opts.prune` every tracking ref the remote no longer holds is
/// deleted. One fetch, one behavior: a tracking ref for a copy the remote
/// no longer has is what makes `ff branch`'s remote section and
/// `Upstream.gone` lie, and the lane and pull share the fetch that keeps
/// them honest.
///
/// Native since the rung was climbed: the negotiation and the pack happen in
/// this process, over the same blocking transport `ff clone` uses. What is
/// still borrowed from git is its config and credential surface, which
/// [`wire_repo`] explains — so this is a claim about the protocol, not about
/// the process table.
///
/// The one exception is the repository gix will not fetch in at all: a
/// half-written worktree admin dir, which [`unopenable_worktree`] names and
/// git walks straight past. There the fetch is handed to `git fetch` once,
/// and only there.
pub fn fetch(cwd: &std::path::Path, remote: &str, opts: &FetchOptions) -> Result<()> {
    let overrides: &[&str] = if opts.interactive {
        &[]
    } else {
        &["gitoxide.credentials.terminalPrompt=false"]
    };
    let repo = wire_repo(cwd, overrides)?;
    let err = match native_fetch(&repo, remote, opts, true) {
        Ok(()) => return Ok(()),
        Err(err) => err,
    };
    let Some(admin) = unopenable_worktree(&repo) else {
        return Err(err);
    };
    // git walks past that directory and fetches; the one spawn buys the
    // user their fetch, and whose process did it is not their problem in
    // the middle of a pull.
    let mut args = vec!["fetch"];
    if opts.prune {
        args.push("--prune");
    }
    if !opts.tags {
        args.push("--no-tags");
    }
    args.push(remote);
    let env: &[(&str, &str)] = if opts.interactive {
        &[]
    } else {
        &[("GIT_TERMINAL_PROMPT", "0")]
    };
    if let Ok(run) = run_env(cwd, &args, env)
        && run.ok
    {
        return Ok(());
    }
    Err(Error::coded(
        "pull/fetch-failed",
        format!(
            "{err}: the worktree admin dir {} is incomplete — git ignores it, \
             fufu's fetch cannot",
            admin.display()
        ),
        vec![
            "ff git worktree prune".into(),
            format!("ff git fetch {remote}"),
            "ff pull --no-fetch".into(),
        ],
    ))
}

/// The native steps — connect, handshake, negotiate, receive, prune — and
/// nothing about the failure lane.
///
/// The transport is built by hand rather than through `Remote::connect`,
/// because the lane's ssh options have nowhere else to go: gix reads
/// `core.sshCommand` with the environment folded in as the last word, so a
/// config override cannot outrank a `GIT_SSH_COMMAND` in the environment,
/// and the options must be set on the transport itself. The connection is
/// then `to_connection_with_transport`, which authenticates through the
/// repository's configured credential helpers exactly as `connect` does.
///
/// `bound_requests` puts the deadline on every http request from the
/// first — the handshake GET, the `ls-refs` POST inside `prepare_fetch`,
/// the fetch POST — through a hook on each request reqwest builds. The
/// hook has one cost: a worker with a request hook refuses to follow a
/// redirect, and the handshake GET is the one request that may (git's
/// `http.followRedirects=initial`). A repository whose URL redirects —
/// `http://` upgraded, a `.git` the host adds — is a permanent fact about
/// that repository, so the refusal is retried once without the hook:
/// each request then rests on reqwest's own thirty-second bound, and the
/// negotiation and pack on the flag as before.
fn native_fetch(
    repo: &gix::Repository,
    remote: &str,
    opts: &FetchOptions,
    bound_requests: bool,
) -> Result<()> {
    use gix::protocol::transport::client::blocking_io::connect;

    let failed = |err: &dyn std::error::Error| {
        Error::coded(
            "pull/fetch-failed",
            format!(
                "fetching from {remote} failed: {}",
                first_useful_line(&chain(err))
            ),
            vec![
                format!("ff git fetch {remote}"),
                "ff pull --no-fetch".into(),
            ],
        )
    };
    let started = std::time::Instant::now();

    let mut remote_handle = repo.find_remote(remote).map_err(|err| failed(&err))?;
    if !opts.tags {
        remote_handle = remote_handle.with_fetch_tags(gix::remote::fetch::Tags::None);
    }
    let (url, version) = remote_handle
        .sanitized_url_and_version(gix::remote::Direction::Fetch)
        .map_err(|err| failed(&err))?;
    let http = matches!(url.scheme, gix::url::Scheme::Http | gix::url::Scheme::Https);

    // The connect half of the deadline, for http: reqwest's own connect
    // timeout is a hard-coded twenty seconds, so a blackholed address is
    // probed here first, under the bound the caller asked for.
    if let Some(deadline) = opts.deadline
        && http
    {
        probe_connect(&url, deadline).map_err(|err| failed(&err))?;
    }

    let mut ssh = if url.scheme == gix::url::Scheme::Ssh {
        repo.ssh_connect_options().map_err(|err| failed(&err))?
    } else {
        Default::default()
    };
    if !opts.interactive && url.scheme == gix::url::Scheme::Ssh {
        quiet_ssh(&mut ssh, opts.deadline);
    }
    let transport = connect::connect(
        url.clone(),
        connect::Options {
            version,
            ssh,
            trace: false,
        },
    )
    .map_err(|err| failed(&err))?;
    let mut connection = remote_handle.to_connection_with_transport(transport);

    // The transfer half of the deadline, for http: reqwest's per-request
    // timeout on every request the worker builds — headers within it, and
    // no single body read stalling longer, so a healthy long pack is not
    // cut. It rides the transport's own options, read from git's config the
    // way gix reads them, with the backend slot filled in.
    if let Some(deadline) = opts.deadline
        && http
        && bound_requests
    {
        use gix::protocol::transport::client::blocking_io::http;
        let url_text = url.to_bstring();
        let mut http_opts = repo
            .transport_options(
                url_text.as_ref() as &gix::bstr::BStr,
                Some(gix::bstr::BStr::new(remote)),
            )
            .map_err(|err| failed(&err))?
            .and_then(|any| any.downcast::<http::Options>().ok())
            .map(|boxed| *boxed)
            .unwrap_or_default();
        let backend = http::reqwest::Options {
            configure_request: Some(Box::new(move |req| {
                *req.timeout_mut() = Some(deadline);
                Ok(())
            })),
        };
        http_opts.backend = Some(std::sync::Arc::new(std::sync::Mutex::new(backend))
            as std::sync::Arc<std::sync::Mutex<dyn std::any::Any + Send + Sync + 'static>>);
        connection = connection.with_transport_options(Box::new(http_opts));
    }

    let prepared = match connection.prepare_fetch(gix::progress::Discard, Default::default()) {
        Ok(prepared) => prepared,
        Err(err) if bound_requests && chain(&err).contains(REDIRECT_REFUSED) => {
            return native_fetch(repo, remote, opts, false);
        }
        Err(err) => return Err(failed(&err)),
    };

    // The negotiation and the pack, under the flag gix checks per round and
    // per read. With a deadline the flag is a local one a timer sets — at
    // the deadline, or when Ctrl-C set the process-wide one, so the signal
    // still lands.
    let received = match opts.deadline {
        None => prepared.receive(gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED),
        Some(deadline) => {
            use std::sync::atomic::{AtomicBool, Ordering};
            let flag = std::sync::Arc::new(AtomicBool::new(false));
            let done = std::sync::Arc::new(AtomicBool::new(false));
            let timer = {
                let flag = flag.clone();
                let done = done.clone();
                std::thread::spawn(move || {
                    while !done.load(Ordering::Relaxed) {
                        if started.elapsed() >= deadline || gix::interrupt::is_triggered() {
                            flag.store(true, Ordering::Relaxed);
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                })
            };
            let received = prepared.receive(gix::progress::Discard, &flag);
            done.store(true, Ordering::Relaxed);
            let _ = timer.join();
            received
        }
    };
    let outcome = match received {
        Ok(outcome) => outcome,
        // A remote with refs and none the refspecs cover — a tags-only
        // remote — has nothing to fetch, which is not a failure to fetch.
        Err(gix::remote::fetch::Error::NoMapping { .. }) => return Ok(()),
        Err(err) => return Err(failed(&err)),
    };

    if opts.prune {
        // An advertisement of nothing is the shape a refspec with no literal
        // prefix produces (ls-refs answers it with zero refs), and pruning
        // against it would delete every tracking ref. A remote that holds
        // nothing keeps its stale refs until it holds something.
        let ref_map = &outcome.ref_map;
        if !ref_map.remote_refs.is_empty() {
            let present: std::collections::HashSet<gix::bstr::BString> = ref_map
                .mappings
                .iter()
                .filter_map(|mapping| mapping.local.clone())
                .collect();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            ff_core::remote::prune_tracking(repo, remote, &present, now)?;
        }
    }
    Ok(())
}

/// What gix's reqwest worker says when a redirect meets a request hook —
/// the one failure [`native_fetch`] retries without the hook.
const REDIRECT_REFUSED: &str = "refusing to follow redirect after request headers were configured";

/// The lane's ssh: no prompt, and a connect that gives up. Only OpenSSH
/// takes these spellings, so any other program — plink, a wrapper script
/// nobody has named the kind of — is left exactly as configured. Naming
/// the kind skips gix's `ssh -G` probe spawn.
///
/// The options ride the command as words, which gix runs through `sh -c`
/// when it may use a shell. With nothing configured gix marks the shell
/// off (its `commandWithoutShellFallback` reading, with no fallback set),
/// so the bare default is given the shell back along with its options; a
/// person who set `gitoxide.ssh.commandWithoutShellFallback` on purpose
/// asked for no shell, and keeps the command they named.
fn quiet_ssh(
    ssh: &mut gix::protocol::transport::client::blocking_io::ssh::connect::Options,
    deadline: Option<std::time::Duration>,
) {
    use gix::protocol::transport::client::blocking_io::ssh::ProgramKind;
    if ssh.disallow_shell && ssh.command.is_some() {
        return;
    }
    let command = ssh.ssh_command().to_os_string();
    let openssh = match ssh.kind {
        Some(kind) => kind == ProgramKind::Ssh,
        None => std::path::Path::new(&command)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem.eq_ignore_ascii_case("ssh")),
    };
    if !openssh {
        return;
    }
    let mut quiet = command;
    quiet.push(" -o BatchMode=yes");
    if let Some(deadline) = deadline {
        quiet.push(format!(" -o ConnectTimeout={}", deadline.as_secs().max(1)));
    }
    ssh.command = Some(quiet);
    ssh.kind = Some(ProgramKind::Ssh);
    ssh.disallow_shell = false;
}

/// Can `url`'s host be reached inside `deadline`? A plain TCP connect, the
/// same one reqwest will make next — this backend has no proxy, so the
/// probe is faithful. Name resolution is not bounded; the connect is.
fn probe_connect(url: &gix::Url, deadline: std::time::Duration) -> std::io::Result<()> {
    use std::net::ToSocketAddrs;
    let host = url
        .host()
        .ok_or_else(|| std::io::Error::other("the URL names no host"))?;
    let port = url.port.unwrap_or(match url.scheme {
        gix::url::Scheme::Https => 443,
        _ => 80,
    });
    let addrs: Vec<_> = (host, port).to_socket_addrs()?.collect();
    let mut last = std::io::Error::other(format!("{host} resolved to no address"));
    let started = std::time::Instant::now();
    for addr in addrs {
        let left = deadline.saturating_sub(started.elapsed());
        if left.is_zero() {
            break;
        }
        match std::net::TcpStream::connect_timeout(&addr, left) {
            Ok(_) => return Ok(()),
            Err(err) => last = err,
        }
    }
    let message = if last.kind() == std::io::ErrorKind::TimedOut {
        format!(
            "no answer from {host}:{port} within {}s",
            deadline.as_secs()
        )
    } else {
        format!("could not connect to {host}:{port}: {last}")
    };
    Err(std::io::Error::new(last.kind(), message))
}

/// The git dir of the first linked worktree gix cannot open as a repository,
/// if any — the admin dir that will have failed the fetch above.
///
/// gix's `receive` looks up the branches checked out in other worktrees so it
/// can decline to move one, and it does that by opening each admin dir
/// `worktrees()` lists — every directory holding a `gitdir` file — through
/// exactly the call below. An admin dir with no readable `commondir` resolves
/// its common dir to itself, so its object dir is `<admin>/objects`, which
/// does not exist, and the open fails. git's own worktree walk skips such a
/// directory; gix's fetch fails on it.
///
/// Making the probe *be* that call, rather than matching on gix's error text
/// or restating why the open fails, is what keeps this exact: whatever gix
/// decides it cannot open is what this reports.
fn unopenable_worktree(repo: &gix::Repository) -> Option<std::path::PathBuf> {
    for proxy in repo.worktrees().ok()? {
        let git_dir = proxy.git_dir().to_path_buf();
        if proxy
            .into_repo_with_possibly_inaccessible_worktree()
            .is_err()
        {
            return Some(git_dir);
        }
    }
    None
}

/// `git push`, in the one of two shapes the plan calls for. A branch with no
/// upstream is created and tracked; an existing one goes under a lease whose
/// expected value is the tip fufu last showed you the shared copy standing
/// at — its own record, not the tracking ref, which any fetch moves. A plan
/// that refused before the wire never gets here.
pub fn push(cwd: &std::path::Path, local_branch: &str, plan: &Push) -> Result<()> {
    let (remote, remote_branch, lease, spec) = match plan {
        Push::Create {
            remote,
            remote_branch,
            ..
        } => (
            remote.clone(),
            remote_branch.clone(),
            None,
            format!("{local_branch}:{remote_branch}"),
        ),
        Push::Push {
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
            "push/unreachable",
            format!(
                "could not reach {remote}: {}",
                first_useful_line(&run.stderr)
            ),
            vec![format!("ff git push {remote}"), "ff status".into()],
        ));
    }
    if run.stderr.contains("stale info") {
        // The exits name the branch: the lease was that branch's, and the
        // branch need not be the one underfoot.
        return Err(Error::coded(
            "push/lease-refused",
            format!(
                "{remote}/{remote_branch} moved since you last looked, so nothing \
                 was pushed — your commits are still here, and ff pull takes in \
                 what arrived"
            ),
            vec![
                format!("ff pull {local_branch}"),
                format!("ff push {local_branch}"),
            ],
        ));
    }
    if run.stderr.contains("[remote rejected]") {
        return Err(Error::coded(
            "push/rejected",
            format!(
                "{remote} refused the push: {}",
                first_useful_line(&run.stderr)
            ),
            vec![format!("ff git push {remote}"), "ff status".into()],
        ));
    }
    Err(Error::coded(
        "push/failed",
        format!(
            "git push to {remote} failed: {}",
            first_useful_line(&run.stderr)
        ),
        vec![format!("ff git push {remote}"), "ff status".into()],
    ))
}

/// The delete half of the same wire: `git push --force-with-lease=<branch>:<lease>
/// <remote> :<branch>`. The lease value is the tip fufu last showed the
/// copy standing at, checked against the tracking ref before the local
/// delete, and git honors it on a delete push, so a move that lands between
/// that check and this send is still caught. The stale-lease case gets its
/// own id because `push/lease-refused` says "your commits are still here,
/// and ff pull takes in what arrived," which is wrong here: the branch is
/// already deleted locally, and the way back is `ff undo`, not a pull.
pub fn push_delete(
    cwd: &std::path::Path,
    remote: &str,
    remote_branch: &str,
    lease: &str,
) -> Result<()> {
    let lease_arg = format!("--force-with-lease={remote_branch}:{lease}");
    let spec = format!(":{remote_branch}");
    let run = run(cwd, &["push", lease_arg.as_str(), remote, spec.as_str()])?;
    if run.ok {
        return Ok(());
    }
    if run.code == 128 {
        return Err(Error::coded(
            "push/unreachable",
            format!(
                "could not reach {remote}: {}",
                first_useful_line(&run.stderr)
            ),
            vec![format!("ff git push {remote}"), "ff status".into()],
        ));
    }
    if run.stderr.contains("stale info") {
        return Err(Error::coded(
            "branch/shared-lease-refused",
            format!(
                "{remote}/{remote_branch} moved since you last looked, so the shared copy \
                 is still there — the branch here is deleted, and ff undo brings it back"
            ),
            vec!["ff undo".into(), "ff branch".into()],
        ));
    }
    if run.stderr.contains("[remote rejected]") {
        return Err(Error::coded(
            "push/rejected",
            format!(
                "{remote} refused the push: {}",
                first_useful_line(&run.stderr)
            ),
            vec![format!("ff git push {remote}"), "ff status".into()],
        ));
    }
    Err(Error::coded(
        "push/failed",
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
            format!("the pack arrived and the working copy could not be written: {err}"),
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

    /// Pull's options: no deadline, prompts allowed, prune, tags.
    const FOREGROUND: FetchOptions = FetchOptions {
        deadline: None,
        interactive: true,
        prune: true,
        tags: true,
    };

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
            &Push::Push {
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
            &Push::Push {
                remote: "origin".into(),
                remote_branch: "main".into(),
                lease: stale.clone(),
                tip: git(&clone1, &["rev-parse", "HEAD"]),
                shape: ff_core::PushShape::Replace,
            },
        );
        let err = res.unwrap_err();
        assert_eq!(err.id(), "push/lease-refused");
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
            &Push::Push {
                remote: "origin".into(),
                remote_branch: "main".into(),
                lease: git(&clone, &["rev-parse", "refs/remotes/origin/main"]),
                tip: git(&clone, &["rev-parse", "HEAD"]),
                shape: ff_core::PushShape::Replace,
            },
        );
        assert_eq!(res.unwrap_err().id(), "push/unreachable");
        let res = fetch(&clone, "origin", &FOREGROUND);
        assert_eq!(res.unwrap_err().id(), "pull/fetch-failed");
    }

    #[test]
    fn a_new_branch_is_created_and_tracked() {
        let (_tmp, remote, clone) = fixture();
        git(&clone, &["checkout", "-b", "feature"]);
        let tip = git(&clone, &["rev-parse", "HEAD"]);
        let res = push(
            &clone,
            "feature",
            &Push::Create {
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
        for plan in [Push::NoRemote, Push::Blocked, Push::UpToDate] {
            assert!(push(&clone, "main", &plan).is_ok());
        }
    }
}
