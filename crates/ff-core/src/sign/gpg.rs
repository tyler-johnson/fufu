//! The two formats that share gpg's interface: `openpgp` through `gpg` and
//! `x509` through `gpgsm`. Both take the payload on stdin and speak the
//! machine-readable status stream, which is what fufu reads rather than the
//! human prose — `[GNUPG:] SIG_CREATED` is what makes a signature a
//! signature, and git requires the same line.

use std::ffi::OsString;

use crate::error::{Error, Result};

use super::verify::{SigStatus, Trust};

pub(super) fn sign(
    repo: &gix::Repository,
    signer: &super::Signer,
    payload: &[u8],
) -> Result<Vec<u8>> {
    let mut args: Vec<OsString> = vec!["--status-fd=2".into(), "-bsa".into()];
    if let Some(key) = &signer.key {
        args.push("-u".into());
        args.push(OsString::from(key));
    }
    let run = super::run(repo, &signer.program, &args, Some(payload))?;
    if !run.ok || !run.stderr.contains("[GNUPG:] SIG_CREATED") {
        return Err(super::failed(&signer.program, &run));
    }
    Ok(run.stdout)
}

/// Verify a detached armored signature over `payload`. The signature goes to
/// a file because that is the only argument gpg takes it in; the payload
/// stays on stdin, where `-` names it.
pub(super) fn verify(
    repo: &gix::Repository,
    program: &str,
    format: super::Format,
    min_trust: Trust,
    payload: &[u8],
    signature: &[u8],
) -> Result<SigStatus> {
    let dir = tempfile::TempDir::new().map_err(Error::repo)?;
    let sig_path = dir.path().join("signature");
    std::fs::write(&sig_path, signature).map_err(Error::repo)?;
    let args: Vec<OsString> = vec![
        "--status-fd=1".into(),
        "--verify".into(),
        sig_path.clone().into(),
        "-".into(),
    ];
    let run = match super::run(repo, program, &args, Some(payload)) {
        Ok(run) => run,
        Err(err) => {
            return Ok(SigStatus {
                present: true,
                format: Some(format.as_str()),
                code: 'E',
                signer: None,
                key: None,
                summary: err.to_string(),
            });
        }
    };
    Ok(parse(
        format,
        min_trust,
        &String::from_utf8_lossy(&run.stdout),
        &run.stderr,
    ))
}

/// Map gpg's status stream onto git's `%G?` alphabet. The status lines are
/// the contract; the prose on stderr is only a fallback summary.
fn parse(format: super::Format, min_trust: Trust, status: &str, stderr: &str) -> SigStatus {
    let mut code = 'E';
    let mut signer = None;
    let mut key = None;
    let mut trust = Trust::Undefined;
    let mut summary: Option<String> = None;

    for line in status.lines() {
        let Some(rest) = line.strip_prefix("[GNUPG:] ") else {
            continue;
        };
        let (tag, tail) = match rest.split_once(' ') {
            Some((tag, tail)) => (tag, tail),
            None => (rest, ""),
        };
        match tag {
            "GOODSIG" | "EXPKEYSIG" | "REVKEYSIG" | "EXPSIG" | "BADSIG" => {
                let (id, who) = match tail.split_once(' ') {
                    Some((id, who)) => (id, who),
                    None => (tail, ""),
                };
                key = Some(id.to_string());
                if !who.is_empty() {
                    signer = Some(who.to_string());
                }
                code = match tag {
                    "GOODSIG" => 'G',
                    "EXPSIG" => 'X',
                    "EXPKEYSIG" => 'Y',
                    "REVKEYSIG" => 'R',
                    _ => 'B',
                };
                // The verdict is the code's job; the summary says who,
                // so the two do not repeat each other where both print.
                let who = signer.clone().or_else(|| key.clone());
                summary = Some(match who {
                    Some(who) => format!("signed by {who}"),
                    None => "signed by a key gpg did not name".to_string(),
                });
            }
            "ERRSIG" => {
                key = tail.split(' ').next().map(str::to_string);
                code = 'E';
                summary = Some("the signature could not be checked".to_string());
            }
            "NO_PUBKEY" => {
                code = 'E';
                summary = Some(format!("no public key for {tail}"));
            }
            "TRUST_UNDEFINED" => trust = Trust::Undefined,
            "TRUST_NEVER" => trust = Trust::Never,
            "TRUST_MARGINAL" => trust = Trust::Marginal,
            "TRUST_FULLY" => trust = Trust::Fully,
            "TRUST_ULTIMATE" => trust = Trust::Ultimate,
            _ => {}
        }
    }

    let summary = summary.unwrap_or_else(|| {
        stderr
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("the signature could not be checked")
            .to_string()
    });

    SigStatus {
        present: true,
        format: Some(format.as_str()),
        code,
        signer,
        key,
        summary,
    }
    .under_trust(trust, min_trust)
}
