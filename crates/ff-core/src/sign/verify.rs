//! Reading a signature back. `ff show` asks for one commit; `ff log
//! --signatures` asks for a page of them, one spawn each, which is why it is
//! a flag rather than the default.
//!
//! The payload handed to the verifier is cut out of the **raw object bytes**
//! rather than rebuilt from a decoded `Commit`: a re-serialization that
//! differs from the original by one byte verifies as a bad signature, and
//! there would be no way to tell that apart from a real one.

use serde::Serialize;

use crate::error::{Error, Result};

/// One commit's signature, in git's own vocabulary.
#[derive(Debug, Clone, Serialize)]
pub struct SigStatus {
    pub present: bool,
    /// `openpgp`, `x509` or `ssh` — read off the armor, not off the config,
    /// because the commit may have been signed elsewhere.
    pub format: Option<&'static str>,
    /// git's `%G?`: `G` good, `B` bad, `U` good but untrusted, `X` expired,
    /// `Y` expired key, `R` revoked key, `E` unverifiable, `N` unsigned.
    pub code: char,
    pub signer: Option<String>,
    pub key: Option<String>,
    pub summary: String,
}

impl SigStatus {
    fn unsigned() -> Self {
        Self {
            present: false,
            format: None,
            code: 'N',
            signer: None,
            key: None,
            summary: "unsigned".to_string(),
        }
    }

    /// A good signature under a trust level below `gpg.minTrustLevel` is
    /// good and untrusted, which is git's `U`. Nothing else is affected.
    pub(super) fn under_trust(mut self, actual: Trust, min: Trust) -> Self {
        if self.code == 'G' && actual < min {
            self.code = 'U';
            self.summary = format!("{} (below gpg.minTrustLevel)", self.summary);
        }
        self
    }

    /// The verdict as a word, for a row or a header. `verified` rather than
    /// git's "good": a signature that checks out has been *verified*, which
    /// is the word every other tool a person meets — GitHub included — puts
    /// on it, and "good" invites the question "good how?".
    pub fn word(&self) -> &'static str {
        match self.code {
            'G' => "verified",
            'B' => "bad signature",
            'U' => "untrusted key",
            'X' => "expired signature",
            'Y' => "expired key",
            'R' => "revoked key",
            'E' => "unverifiable",
            _ => "unsigned",
        }
    }

    /// The signing tool, as a person names it rather than as `gpg.format`
    /// spells it: `openpgp` is the format, `gpg` is the thing you ran.
    pub fn tool(&self) -> &'static str {
        match self.format {
            Some("openpgp") => "gpg",
            Some("x509") => "gpgsm",
            Some("ssh") => "ssh",
            _ => "unknown",
        }
    }

    /// The key, shortened to the eight characters that identify it at a
    /// glance — the same width fufu shortens a sha to. An ssh fingerprint is
    /// `SHA256:` and base64 rather than hex, so the prefix comes off and the
    /// digest is cut in the same place.
    pub fn short_key(&self) -> Option<String> {
        let key = self.key.as_deref()?;
        let body = key.strip_prefix("SHA256:").unwrap_or(key);
        Some(if key.starts_with("SHA256:") {
            body.chars().take(8).collect()
        } else {
            // A gpg key id identifies from its tail, which is why gpg's own
            // short form is the last eight.
            let chars: Vec<char> = body.chars().collect();
            chars[chars.len().saturating_sub(8)..].iter().collect()
        })
    }
}

/// gpg's trust ladder, ordered so a comparison means what it reads like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Trust {
    Undefined,
    Never,
    Marginal,
    Fully,
    Ultimate,
}

impl Trust {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "undefined" => Some(Trust::Undefined),
            "never" => Some(Trust::Never),
            "marginal" => Some(Trust::Marginal),
            "fully" => Some(Trust::Fully),
            "ultimate" => Some(Trust::Ultimate),
            _ => None,
        }
    }
}

/// Whether a raw commit carries a signature at all — a scan of the header
/// block, no spawn and no verifier.
///
/// This is the cheap half of the question, and the reason `ff log` can say
/// "signed" on every row it prints without costing anything: *carrying* a
/// signature is a fact about the object, while *verifying* one is a process.
/// The two are worth different words, and only the second is worth a flag.
pub fn has_signature(raw: &[u8]) -> bool {
    let Some(blank) = raw.windows(2).position(|pair| pair == b"\n\n") else {
        return false;
    };
    raw[..blank + 1]
        .split_inclusive(|&byte| byte == b'\n')
        .any(is_signature_header)
}

/// The `gpgsig` header line, in either spelling. `gpgsig-sha256` belongs to
/// dual-hash compat objects that fufu never writes but may well read.
fn is_signature_header(line: &[u8]) -> bool {
    line.starts_with(b"gpgsig ") || line.starts_with(b"gpgsig-sha256 ")
}

/// Everything a verification needs that does not come off the commit: the
/// child environment, the three programs, and the trust configuration.
///
/// Resolved once and then read-only, which is what makes a batch of
/// verifications safe to run on several threads — `gix::Repository` is not
/// `Sync`, so nothing past this struct may touch it.
struct Verifier {
    ctx: gix::command::Context,
    openpgp: String,
    x509: String,
    ssh: String,
    min_trust: Trust,
    allowed: Option<String>,
    revocations: Option<String>,
}

impl Verifier {
    fn new(repo: &gix::Repository) -> Result<Self> {
        let ctx = super::context(repo)?;
        let snap = repo.config_snapshot();
        Ok(Self {
            ctx,
            openpgp: super::program_of(&snap, super::Format::OpenPgp),
            x509: super::program_of(&snap, super::Format::X509),
            ssh: super::program_of(&snap, super::Format::Ssh),
            min_trust: snap
                .string("gpg.minTrustLevel")
                .and_then(|raw| Trust::parse(&raw.to_string()))
                .unwrap_or(Trust::Undefined),
            allowed: super::allowed_signers(&snap),
            revocations: super::revocation_file(repo),
        })
    }

    /// One verification. Spawns; touches no repository.
    fn check(&self, payload: &[u8], signature: &[u8]) -> SigStatus {
        let armor = String::from_utf8_lossy(signature);
        let format = if armor.contains("BEGIN PGP SIGNATURE") {
            super::Format::OpenPgp
        } else if armor.contains("BEGIN SIGNED MESSAGE") {
            super::Format::X509
        } else if armor.contains("BEGIN SSH SIGNATURE") {
            super::Format::Ssh
        } else {
            return SigStatus {
                present: true,
                format: None,
                code: 'E',
                signer: None,
                key: None,
                summary: "the gpgsig header is not an armored signature fufu recognizes"
                    .to_string(),
            };
        };
        let result = match format {
            super::Format::Ssh => super::ssh::verify(
                &self.ctx,
                &self.ssh,
                self.allowed.as_deref(),
                self.revocations.as_deref(),
                self.min_trust,
                payload,
                signature,
            ),
            super::Format::OpenPgp => super::gpg::verify(
                &self.ctx,
                &self.openpgp,
                format,
                self.min_trust,
                payload,
                signature,
            ),
            super::Format::X509 => super::gpg::verify(
                &self.ctx,
                &self.x509,
                format,
                self.min_trust,
                payload,
                signature,
            ),
        };
        // A verifier that could not be run is an unverifiable signature,
        // not a failed command: the commit is still a commit.
        result.unwrap_or_else(|err| SigStatus {
            present: true,
            format: Some(format.as_str()),
            code: 'E',
            signer: None,
            key: None,
            summary: err.to_string(),
        })
    }
}

/// Verify one commit's signature, spawning the verifier the armor asks for.
/// An unsigned commit is `N` and costs no spawn.
pub fn verify(repo: &gix::Repository, id: gix::ObjectId) -> Result<SigStatus> {
    let object = repo.find_object(id).map_err(Error::repo)?;
    let Some((payload, signature)) = split(&object.data) else {
        return Ok(SigStatus::unsigned());
    };
    Ok(Verifier::new(repo)?.check(&payload, &signature))
}

/// Verify a page of commits, one status per id in the order given.
///
/// The reads and the splits happen here, on this thread and in-process; only
/// the spawns fan out. That split is the whole design — it keeps the
/// repository on one thread, and the part worth parallelizing is the part
/// that is nearly all process startup and waiting.
///
/// Verification only. Signing is never run this way: it can prompt for a
/// passphrase, and several pinentries racing for one terminal is not a
/// speed-up.
pub fn verify_many(repo: &gix::Repository, ids: &[gix::ObjectId]) -> Result<Vec<SigStatus>> {
    let mut jobs: Vec<Option<(Vec<u8>, Vec<u8>)>> = Vec::with_capacity(ids.len());
    for &id in ids {
        let object = repo.find_object(id).map_err(Error::repo)?;
        jobs.push(split(&object.data));
    }
    let signed = jobs.iter().filter(|job| job.is_some()).count();
    if signed == 0 {
        return Ok(jobs.iter().map(|_| SigStatus::unsigned()).collect());
    }

    let verifier = Verifier::new(repo)?;
    let mut out: Vec<SigStatus> = jobs.iter().map(|_| SigStatus::unsigned()).collect();
    let workers = workers(signed);
    if workers <= 1 {
        for (job, slot) in jobs.iter().zip(out.iter_mut()) {
            if let Some((payload, signature)) = job {
                *slot = verifier.check(payload, signature);
            }
        }
        return Ok(out);
    }

    // Chunked rather than a work queue: the jobs are one process spawn
    // apiece, so they cost about the same and an atomic cursor would buy
    // nothing but contention.
    let chunk = jobs.len().div_ceil(workers);
    let verifier = &verifier;
    std::thread::scope(|scope| {
        for (jobs, slots) in jobs.chunks(chunk).zip(out.chunks_mut(chunk)) {
            scope.spawn(move || {
                for (job, slot) in jobs.iter().zip(slots.iter_mut()) {
                    if let Some((payload, signature)) = job {
                        *slot = verifier.check(payload, signature);
                    }
                }
            });
        }
    });
    Ok(out)
}

/// How many verifiers to run at once. Capped well below what a big machine
/// would allow: past a handful these queue on gpg-agent rather than on the
/// CPU, and a page of log is a few dozen rows at most.
fn workers(jobs: usize) -> usize {
    const CAP: usize = 8;
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    jobs.min(cpus).clamp(1, CAP)
}

/// Cut a raw commit into what was signed and the signature that was over it:
/// the `gpgsig` header's folded value, unfolded, and the object with those
/// lines removed. `None` when the commit carries no signature.
fn split(raw: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    // The header block ends at the first blank line. A folded signature's
    // own blank line is a lone space, never empty, so this cannot land
    // inside one.
    let blank = raw.windows(2).position(|pair| pair == b"\n\n")?;
    let header_end = blank + 1;

    let mut payload = Vec::with_capacity(raw.len());
    let mut signature = Vec::new();
    let mut in_signature = false;
    let mut found = false;
    let mut pos = 0usize;
    while pos < header_end {
        let end = raw[pos..header_end]
            .iter()
            .position(|&byte| byte == b'\n')
            .map(|offset| pos + offset + 1)?;
        let line = &raw[pos..end];
        if in_signature && line.first() == Some(&b' ') {
            signature.extend_from_slice(&line[1..]);
        } else {
            in_signature = false;
            match line.iter().position(|&byte| byte == b' ') {
                Some(at) if !found && is_signature_header(line) => {
                    signature.extend_from_slice(&line[at + 1..]);
                    in_signature = true;
                    found = true;
                }
                _ => payload.extend_from_slice(line),
            }
        }
        pos = end;
    }
    if !found {
        return None;
    }
    payload.extend_from_slice(&raw[header_end..]);
    Some((payload, signature))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIGNED: &[u8] = concat!(
        "tree 4b825dc642cb6eb9a060e54bf8d69288fbee4904\n",
        "author A <a@b> 1 +0000\n",
        "committer A <a@b> 1 +0000\n",
        "gpgsig -----BEGIN SSH SIGNATURE-----\n",
        " \n",
        " AAAA\n",
        " -----END SSH SIGNATURE-----\n",
        "\n",
        "the message\n",
    )
    .as_bytes();

    #[test]
    fn the_signature_comes_out_unfolded_and_the_payload_without_it() {
        let (payload, signature) = split(SIGNED).expect("a signature");
        assert_eq!(
            String::from_utf8_lossy(&signature),
            "-----BEGIN SSH SIGNATURE-----\n\nAAAA\n-----END SSH SIGNATURE-----\n"
        );
        assert_eq!(
            String::from_utf8_lossy(&payload),
            concat!(
                "tree 4b825dc642cb6eb9a060e54bf8d69288fbee4904\n",
                "author A <a@b> 1 +0000\n",
                "committer A <a@b> 1 +0000\n",
                "\n",
                "the message\n",
            )
        );
    }

    #[test]
    fn an_unsigned_commit_splits_into_nothing() {
        let raw = concat!(
            "tree 4b825dc642cb6eb9a060e54bf8d69288fbee4904\n",
            "author A <a@b> 1 +0000\n",
            "committer A <a@b> 1 +0000\n",
            "\n",
            "the message\n",
        )
        .as_bytes();
        assert!(split(raw).is_none());
    }

    #[test]
    fn a_good_signature_below_the_floor_is_untrusted() {
        let status = SigStatus {
            present: true,
            format: Some("openpgp"),
            code: 'G',
            signer: None,
            key: None,
            summary: "good signature".into(),
        }
        .under_trust(Trust::Marginal, Trust::Fully);
        assert_eq!(status.code, 'U');
    }
}
