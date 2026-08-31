//! Commit signing. gix implements none, so fufu spawns the signer itself —
//! `gpg`, `gpgsm`, or `ssh-keygen`, whichever `gpg.format` names — exactly as
//! git does, and reads git's own configuration keys to decide. Nothing here
//! is a `fufu.*` setting: a repository that already signs under git signs
//! under fufu without being told twice.
//!
//! Two entry points, deliberately split. [`enabled`] reads `commit.gpgsign`
//! and nothing else, and never spawns — it is what a render asks. [`resolve`]
//! does the full resolution, may run `gpg.ssh.defaultKeyCommand`, and is
//! called once per verb *before* the first object is written, so a
//! misconfiguration aborts with nothing on disk.
//!
//! Every user commit fufu writes goes through [`write_user_commit`]: the
//! close, and every commit a rewrite replays. Machinery commits do not —
//! the op journal carries fufu's own identity rather than the user's, and
//! park/stash commits are internal scratch that never leaves the repository,
//! which is `git stash`'s position too.
//!
//! These are sanctioned spawns, like the user's commit hooks.

use std::ffi::OsString;
use std::io::Write as _;
use std::process::Stdio;

use gix::bstr::BString;

use crate::error::{Error, Result};

mod gpg;
mod ssh;
pub mod verify;

/// Which signing tool `gpg.format` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    OpenPgp,
    X509,
    Ssh,
}

impl Format {
    pub fn as_str(self) -> &'static str {
        match self {
            Format::OpenPgp => "openpgp",
            Format::X509 => "x509",
            Format::Ssh => "ssh",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "openpgp" => Some(Format::OpenPgp),
            "x509" => Some(Format::X509),
            "ssh" => Some(Format::Ssh),
            _ => None,
        }
    }

    fn default_program(self) -> &'static str {
        match self {
            Format::OpenPgp => "gpg",
            Format::X509 => "gpgsm",
            Format::Ssh => "ssh-keygen",
        }
    }
}

/// Whether this invocation signs. `Config` is what every verb but `ff commit`
/// passes; the other two are `-S` and `--no-sign`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Choice {
    #[default]
    Config,
    Force,
    Skip,
}

/// What the repository's signing configuration says, read without spawning
/// anything. `ff doctor` reports this; [`resolve`] turns it into a signer.
pub struct Setup {
    /// `commit.gpgsign`.
    pub on: bool,
    /// `gpg.format` as written, so an unknown value can be quoted back.
    pub raw_format: String,
    /// `None` when `gpg.format` names something fufu does not sign with.
    pub format: Option<Format>,
    /// The program that format would run. Empty when the format is unknown.
    pub program: String,
    /// `user.signingkey`.
    pub key: Option<String>,
    /// `gpg.ssh.defaultKeyCommand` — a spawn, so only [`resolve`] runs it.
    pub default_key_command: Option<String>,
    /// `gpg.ssh.allowedSignersFile`: signing works without it, verification
    /// does not.
    pub allowed_signers: Option<String>,
}

/// A resolved signer: everything the spawn needs, worked out once.
pub struct Signer {
    pub format: Format,
    pub program: String,
    pub key: Option<String>,
    /// The environment git hands a spawned child. Held rather than derived
    /// per call so [`run`] never needs the repository — which is what lets
    /// verification fan out across threads, `Repository` being `!Sync`.
    ctx: gix::command::Context,
}

/// Whether this repository asks for signed commits. Reads one key and never
/// spawns, which is what makes it safe on a render path.
pub fn enabled(repo: &gix::Repository) -> bool {
    repo.config_snapshot()
        .boolean("commit.gpgsign")
        .unwrap_or(false)
}

/// The signing configuration, read-only and spawn-free.
pub fn setup(repo: &gix::Repository) -> Setup {
    let snap = repo.config_snapshot();
    let raw_format = snap
        .string("gpg.format")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "openpgp".to_string());
    let format = Format::parse(&raw_format);
    Setup {
        on: snap.boolean("commit.gpgsign").unwrap_or(false),
        program: format.map(|f| program_of(&snap, f)).unwrap_or_default(),
        raw_format,
        format,
        key: snap
            .string("user.signingkey")
            .map(|v| v.to_string())
            .filter(|v| !v.is_empty()),
        default_key_command: snap
            .string("gpg.ssh.defaultKeyCommand")
            .map(|v| v.to_string())
            .filter(|v| !v.is_empty()),
        allowed_signers: allowed_signers(&snap),
    }
}

/// The program one format runs. `gpg.program` is the openpgp alias git
/// carries from before the format axis existed, so the format-specific key
/// wins over it.
fn program_of(snap: &gix::config::Snapshot<'_>, format: Format) -> String {
    let key = match format {
        Format::OpenPgp => "gpg.openpgp.program",
        Format::X509 => "gpg.x509.program",
        Format::Ssh => "gpg.ssh.program",
    };
    if let Some(value) = snap.string(key)
        && !value.is_empty()
    {
        return value.to_string();
    }
    if format == Format::OpenPgp
        && let Some(value) = snap.string("gpg.program")
        && !value.is_empty()
    {
        return value.to_string();
    }
    format.default_program().to_string()
}

fn allowed_signers(snap: &gix::config::Snapshot<'_>) -> Option<String> {
    match snap.trusted_path("gpg.ssh.allowedSignersFile") {
        Some(Ok(path)) => Some(path.display().to_string()),
        _ => None,
    }
}

fn revocation_file(repo: &gix::Repository) -> Option<String> {
    match repo
        .config_snapshot()
        .trusted_path("gpg.ssh.revocationFile")
    {
        Some(Ok(path)) => Some(path.display().to_string()),
        _ => None,
    }
}

/// Resolve the signer for one invocation, or `None` when this invocation does
/// not sign. Called once per verb, before the first object is written: a bad
/// `gpg.format` or a missing key must cost nothing.
pub fn resolve(repo: &gix::Repository, choice: Choice) -> Result<Option<Signer>> {
    let want = match choice {
        Choice::Skip => return Ok(None),
        Choice::Force => true,
        Choice::Config => enabled(repo),
    };
    if !want {
        return Ok(None);
    }
    let ctx = context(repo)?;
    let setup = setup(repo);
    let Some(format) = setup.format else {
        return Err(Error::coded(
            "sign/unknown-format",
            format!(
                "gpg.format is \"{}\": fufu signs with openpgp, x509 or ssh",
                setup.raw_format
            ),
            vec![
                "git config gpg.format ssh".into(),
                "git config --unset commit.gpgsign".into(),
            ],
        ));
    };
    let mut key = setup.key;
    if key.is_none()
        && format == Format::Ssh
        && let Some(command) = &setup.default_key_command
    {
        key = ssh::default_key(&ctx, command)?;
    }
    if format == Format::Ssh && key.is_none() {
        return Err(Error::coded(
            "sign/no-key",
            "ssh signing needs a key: set user.signingkey to the key to sign with",
            vec![
                "git config user.signingkey ~/.ssh/id_ed25519.pub".into(),
                "ff commit --no-sign".into(),
            ],
        ));
    }
    Ok(Some(Signer {
        format,
        program: setup.program,
        key,
        ctx,
    }))
}

/// Write a user commit, carrying a `gpgsig` header when the repository asks
/// for one. The payload signed is the commit object without that header —
/// serialize, sign, then push the header on last.
///
/// gix serializes `extra_headers` after everything else and folds a
/// multi-line value with the leading space git's decoder unfolds, so a raw
/// armored block stored here lands byte-identical to what git writes.
pub(crate) fn write_user_commit(
    repo: &gix::Repository,
    signer: Option<&Signer>,
    mut commit: gix::objs::Commit,
) -> Result<gix::ObjectId> {
    if let Some(signer) = signer {
        use gix::objs::WriteTo as _;
        let mut payload = Vec::new();
        commit.write_to(&mut payload).map_err(Error::repo)?;
        let mut armored = signer.sign(&payload)?;
        if !armored.ends_with(b"\n") {
            armored.push(b'\n');
        }
        // Always `gpgsig`. `gpgsig-sha256` belongs to dual-hash compat
        // objects, which fufu never writes — the strip on replay still
        // matches both, because it may be reading one git wrote.
        commit
            .extra_headers
            .push((BString::from("gpgsig"), BString::from(armored)));
    }
    Ok(repo.write_object(&commit).map_err(Error::repo)?.detach())
}

impl Signer {
    fn sign(&self, payload: &[u8]) -> Result<Vec<u8>> {
        match self.format {
            Format::OpenPgp | Format::X509 => gpg::sign(self, payload),
            Format::Ssh => ssh::sign(self, payload),
        }
    }
}

/// The environment git gives a child process, with stderr left alone.
///
/// The context's own default is "inherit stderr", and it is applied *after*
/// the pipes a spawn sets — leaving it set would take the signer's diagnosis
/// with it, which is the whole reason stderr is captured.
pub(crate) fn context(repo: &gix::Repository) -> Result<gix::command::Context> {
    let mut ctx = repo.command_context().map_err(Error::repo)?;
    ctx.stderr = None;
    Ok(ctx)
}

/// The first word of a program string, when the string is a plain program
/// path rather than a shell fragment. `None` means "not ours to
/// second-guess" — the spawn will say.
fn program_name(program: &str) -> Option<&str> {
    const SHELLY: &[char] = &[
        '|', '&', ';', '<', '>', '(', ')', '$', '`', '\\', '"', '\'', '*', '?', '[', '#', '~', '=',
        '%',
    ];
    if program.contains(SHELLY) {
        return None;
    }
    program.split_whitespace().next()
}

/// Whether a configured signer program can be found. Stats files; never
/// spawns, so `ff doctor` can ask.
pub fn program_available(program: &str) -> bool {
    let Some(name) = program_name(program) else {
        return true;
    };
    let path = std::path::Path::new(name);
    if name.contains('/') || name.contains('\\') {
        return path.is_file();
    }
    let Some(var) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&var).any(|dir| {
        if dir.as_os_str().is_empty() {
            return false;
        }
        if dir.join(name).is_file() {
            return true;
        }
        cfg!(windows)
            && ["exe", "cmd", "bat"]
                .iter()
                .any(|ext| dir.join(format!("{name}.{ext}")).is_file())
    })
}

struct Run {
    ok: bool,
    stdout: Vec<u8>,
    stderr: String,
}

/// Run one signer, capturing both streams.
///
/// stderr is captured rather than inherited on purpose: gpg-agent's pinentry
/// opens `/dev/tty` itself through `GPG_TTY`, so a passphrase prompt still
/// reaches the terminal, and capturing buys a failure message worth printing
/// — the same trade the push lane makes.
fn run(
    ctx: &gix::command::Context,
    program: &str,
    args: &[OsString],
    input: Option<&[u8]>,
) -> Result<Run> {
    if !program_available(program) {
        return Err(no_program(program));
    }
    let mut prepare = gix::command::prepare(program)
        .with_shell()
        .with_context(ctx.clone());
    prepare.args = args.to_vec();
    let mut cmd: std::process::Command = prepare.into();
    cmd.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|_| no_program(program))?;
    if let Some(bytes) = input
        && let Some(mut stdin) = child.stdin.take()
    {
        // A signer that died before reading gets an EPIPE here; its exit
        // status and stderr say more about why than this write does.
        let _ = stdin.write_all(bytes);
    }
    let out = child.wait_with_output().map_err(Error::repo)?;
    Ok(Run {
        ok: out.status.success(),
        stdout: out.stdout,
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

fn no_program(program: &str) -> Error {
    Error::coded(
        "sign/no-program",
        format!("{program} is not on PATH, and fufu spawns it to sign commits"),
        vec![
            "ff doctor".into(),
            "git config --unset commit.gpgsign".into(),
        ],
    )
}

/// A signer that ran and refused. Its own words carry the reason, minus the
/// machine-readable status stream nobody wants to read.
fn failed(program: &str, run: &Run) -> Error {
    let detail: Vec<&str> = run
        .stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("[GNUPG:]"))
        .collect();
    let message = if detail.is_empty() {
        format!("{program} did not sign the commit")
    } else {
        format!("{program} did not sign the commit: {}", detail.join("; "))
    };
    Error::coded(
        "sign/failed",
        message,
        vec!["ff doctor".into(), "ff commit --no-sign".into()],
    )
}
