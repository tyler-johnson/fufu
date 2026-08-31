//! `ssh` format signing, through `ssh-keygen -Y`. Everything moves through
//! files here — ssh-keygen takes the payload as a path and writes the
//! signature beside it — so a temporary directory is the whole mechanism.
//!
//! `user.signingkey` is mandatory for this format, or `gpg.ssh.defaultKeyCommand`
//! must produce one. A literal key (`ssh-ed25519 AAAA…`, or the `key::`
//! spelling git's default-key protocol uses) is written to a file and signed
//! through the agent, which is what `-U` means.

use std::ffi::OsString;

use crate::error::{Error, Result};

use super::verify::{SigStatus, Trust};

/// The git namespace every git ssh signature is made under. Wrong namespace,
/// no verification — so it is the same constant on both sides.
const NAMESPACE: &str = "git";

pub(super) fn sign(
    repo: &gix::Repository,
    signer: &super::Signer,
    payload: &[u8],
) -> Result<Vec<u8>> {
    let key = signer
        .key
        .as_deref()
        .ok_or_else(|| Error::msg("ssh signing without a key: resolution should have refused"))?;
    let dir = tempfile::TempDir::new().map_err(Error::repo)?;

    let mut args: Vec<OsString> = vec![
        "-Y".into(),
        "sign".into(),
        "-n".into(),
        NAMESPACE.into(),
        "-f".into(),
    ];
    if let Some(literal) = literal_key(key) {
        let key_path = dir.path().join("signing_key.pub");
        std::fs::write(&key_path, literal).map_err(Error::repo)?;
        args.push(key_path.into());
        // The file holds a public key, so the private half has to come from
        // the agent.
        args.push("-U".into());
    } else {
        args.push(OsString::from(key));
    }

    let payload_path = dir.path().join("payload");
    std::fs::write(&payload_path, payload).map_err(Error::repo)?;
    args.push(payload_path.clone().into());

    let run = super::run(repo, &signer.program, &args, None)?;
    if !run.ok {
        return Err(super::failed(&signer.program, &run));
    }
    // ssh-keygen writes `<payload>.sig` beside the file it signed.
    let mut sig_path = payload_path.into_os_string();
    sig_path.push(".sig");
    std::fs::read(std::path::PathBuf::from(sig_path)).map_err(|err| {
        Error::coded(
            "sign/failed",
            format!("{} wrote no signature: {err}", signer.program),
            vec!["ff doctor".into(), "ff commit --no-sign".into()],
        )
    })
}

/// A `user.signingkey` that is the key itself rather than a path to one,
/// normalized to what belongs in a file.
fn literal_key(key: &str) -> Option<String> {
    let body = key.strip_prefix("key::").unwrap_or(key);
    let looks_literal = key.starts_with("key::")
        || body.starts_with("ssh-")
        || body.starts_with("ecdsa-")
        || body.starts_with("sk-");
    if !looks_literal {
        return None;
    }
    let mut text = body.to_string();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    Some(text)
}

/// `gpg.ssh.defaultKeyCommand`: run it and take its first line, which git's
/// protocol says is the key (usually behind a `key::` prefix). A command that
/// fails is not an error — it is simply no key, and the caller's refusal is
/// the one worth reading.
pub(super) fn default_key(repo: &gix::Repository, command: &str) -> Result<Option<String>> {
    let run = super::run(repo, command, &[], None)?;
    if !run.ok {
        return Ok(None);
    }
    Ok(String::from_utf8_lossy(&run.stdout)
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string))
}

/// Verify an ssh signature: two spawns, because ssh-keygen will not tell you
/// who signed something and check the signature in one call. Without
/// `gpg.ssh.allowedSignersFile` there is nothing to check against, and that
/// is `E` — unverifiable, not bad.
pub(super) fn verify(
    repo: &gix::Repository,
    program: &str,
    allowed: Option<&str>,
    revocations: Option<&str>,
    min_trust: Trust,
    payload: &[u8],
    signature: &[u8],
) -> Result<SigStatus> {
    let unchecked = |summary: String| SigStatus {
        present: true,
        format: Some(super::Format::Ssh.as_str()),
        code: 'E',
        signer: None,
        key: None,
        summary,
    };
    let Some(allowed) = allowed else {
        return Ok(unchecked(
            "no gpg.ssh.allowedSignersFile, so there is nothing to check this against".to_string(),
        ));
    };

    let dir = tempfile::TempDir::new().map_err(Error::repo)?;
    let sig_path = dir.path().join("signature");
    std::fs::write(&sig_path, signature).map_err(Error::repo)?;

    let find: Vec<OsString> = vec![
        "-Y".into(),
        "find-principals".into(),
        "-f".into(),
        OsString::from(allowed),
        "-s".into(),
        sig_path.clone().into(),
    ];
    let found = match super::run(repo, program, &find, None) {
        Ok(run) => run,
        Err(err) => return Ok(unchecked(err.to_string())),
    };
    let principal = String::from_utf8_lossy(&found.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string);
    let Some(principal) = principal.filter(|_| found.ok) else {
        return Ok(unchecked(
            "no principal in gpg.ssh.allowedSignersFile matches the signing key".to_string(),
        ));
    };

    let mut check: Vec<OsString> = vec![
        "-Y".into(),
        "verify".into(),
        "-n".into(),
        NAMESPACE.into(),
        "-f".into(),
        OsString::from(allowed),
        "-I".into(),
        OsString::from(&principal),
        "-s".into(),
        sig_path.into(),
    ];
    if let Some(revocations) = revocations {
        check.push("-r".into());
        check.push(OsString::from(revocations));
    }
    let checked = match super::run(repo, program, &check, Some(payload)) {
        Ok(run) => run,
        Err(err) => return Ok(unchecked(err.to_string())),
    };
    // ssh-keygen writes its verdict to stderr, and the key fingerprint with
    // it — the one place the key id can be had.
    let said = checked
        .stderr
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .to_string();
    let key = said
        .split_whitespace()
        .find(|word| word.starts_with("SHA256:"))
        .map(str::to_string);

    let summary = if checked.ok {
        match &key {
            Some(key) => format!("signed by {principal} with {key}"),
            None => format!("signed by {principal}"),
        }
    } else if said.is_empty() {
        format!("{program} rejected the signature")
    } else {
        said
    };
    let status = SigStatus {
        present: true,
        format: Some(super::Format::Ssh.as_str()),
        code: if checked.ok { 'G' } else { 'B' },
        signer: Some(principal),
        key,
        summary,
    };
    // ssh has no web of trust to consult: a signature by a key the allowed
    // signers file names is as trusted as the file is, which is git's
    // reading too — so `gpg.minTrustLevel` only bites at `ultimate`.
    Ok(status.under_trust(Trust::Fully, min_trust))
}
