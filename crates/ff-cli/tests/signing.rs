//! `ff commit` and the signing configuration, driven through a stub signer.
//!
//! The stub is a `#!/bin/sh` script that emits a fixed armored block and the
//! `[GNUPG:] SIG_CREATED` status line gpg emits — enough to be a signer for
//! everything on this side of the boundary, and nothing that needs a keyring.
//! What a real signature has to survive is `crates/ff-core/tests/diff_sign.rs`,
//! which signs with ssh and hands the result to git's own verifier.
//!
//! unix-only, for the same reason `zero_spawn.rs` is: the stub is a shell
//! script, and Windows cannot execute one.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ff_testsupport::Fixture;
use ff_testsupport::fixtures::null_device;

/// A stub signer script, kept alive by its temp directory.
struct Stub {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

impl Stub {
    fn config_value(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

/// A signer that signs: it drains the payload, writes gpg's status line to
/// fd 2 (where `--status-fd=2` puts it), and prints an armored block.
fn good_signer() -> Stub {
    stub(
        "#!/bin/sh\n\
         cat > /dev/null\n\
         echo '[GNUPG:] SIG_CREATED D 1 8 00 0 0' >&2\n\
         printf -- '-----BEGIN PGP SIGNATURE-----\\n\\nc3R1Yg==\\n-----END PGP SIGNATURE-----\\n'\n\
         exit 0\n",
    )
}

/// A signer that refuses, the way gpg refuses when it cannot reach a key.
fn failing_signer() -> Stub {
    stub(
        "#!/bin/sh\n\
         cat > /dev/null\n\
         echo 'gpg: signing failed: No secret key' >&2\n\
         exit 2\n",
    )
}

fn stub(script: &str) -> Stub {
    let dir = tempfile::TempDir::new().expect("stub dir");
    let path = dir.path().join("stub-signer");
    std::fs::write(&path, script).expect("write stub");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod stub");
    Stub { _dir: dir, path }
}

fn ff_at(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("spawn ff")
}

fn ff(fx: &Fixture, args: &[&str]) -> Output {
    ff_at(&fx.path(), args)
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn json(out: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(out)).expect("valid json")
}

/// A repository with an identity fufu can commit under, and a stub signer
/// named as `gpg.program`. Signing itself is left off — each test says
/// whether it wants it.
fn repo_with(signer: &Stub) -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Fixture Committer");
    fx.set_config("user.email", "committer@fixture.test");
    fx.set_config("gpg.program", &signer.config_value());
    fx
}

fn raw_head(fx: &Fixture) -> String {
    fx.git(&["cat-file", "commit", "HEAD"])
}

/// `commit.gpgsign` alone is enough: no flag, no fufu setting, git's key.
#[test]
fn commit_gpgsign_puts_a_gpgsig_header_on_the_commit() {
    let signer = good_signer();
    let fx = repo_with(&signer);
    fx.set_config("commit.gpgsign", "true");
    fx.write("a.txt", "one\n");

    let out = ff(&fx, &["commit", "-m", "signed"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        raw_head(&fx).contains("gpgsig -----BEGIN PGP SIGNATURE-----"),
        "no gpgsig header:\n{}",
        raw_head(&fx)
    );
}

/// Without the setting, nothing is signed and nothing is spawned.
#[test]
fn an_unconfigured_repository_signs_nothing() {
    let signer = good_signer();
    let fx = repo_with(&signer);
    fx.write("a.txt", "one\n");

    let out = ff(&fx, &["commit", "-m", "plain"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(!raw_head(&fx).contains("gpgsig"));
}

/// `-S` signs a repository that does not, and `--no-sign` declines to sign
/// one that does. The flags are booleans on purpose: `ff commit` takes
/// positional paths, so git's `-S<keyid>` would make `ff commit -S file.txt`
/// ambiguous. The key is always `user.signingkey`.
#[test]
fn the_flags_override_the_configuration_both_ways() {
    let signer = good_signer();

    let fx = repo_with(&signer);
    fx.write("a.txt", "one\n");
    let out = ff(&fx, &["commit", "-S", "-m", "forced"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(raw_head(&fx).contains("gpgsig"), "-S did not sign");

    let fx = repo_with(&signer);
    fx.set_config("commit.gpgsign", "true");
    fx.write("a.txt", "one\n");
    let out = ff(&fx, &["commit", "--no-sign", "-m", "skipped"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(!raw_head(&fx).contains("gpgsig"), "--no-sign signed anyway");
}

/// A signer that refuses aborts the close, and the branch does not move. The
/// commit object is written before the journal append and before any ref
/// moves, so a refusal leaves an unreferenced object and nothing else.
#[test]
fn a_signer_that_refuses_aborts_the_close_and_moves_no_ref() {
    let signer = failing_signer();
    let fx = repo_with(&signer);
    // The base commit is git's, and git reads the same two keys — so signing
    // goes on only once there is a branch to fail to move.
    fx.write("base.txt", "base\n");
    fx.commit("base");
    let before = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    fx.set_config("commit.gpgsign", "true");

    fx.write("a.txt", "one\n");
    let out = ff(&fx, &["commit", "-m", "refused"]);
    assert!(!out.status.success(), "the close should have failed");
    assert!(
        stderr(&out).contains("No secret key"),
        "the signer's own words did not survive: {}",
        stderr(&out)
    );
    assert_eq!(
        fx.git(&["rev-parse", "HEAD"]).trim(),
        before,
        "the branch moved despite a failed signature"
    );
    // The tree is still open: nothing about the change was consumed.
    assert!(stdout(&ff(&fx, &["status"])).contains("a.txt"));
}

/// An unknown `gpg.format` is refused before anything is written, with an id
/// `ff explain` knows.
#[test]
fn an_unknown_gpg_format_is_a_coded_refusal() {
    let signer = good_signer();
    let fx = repo_with(&signer);
    fx.set_config("commit.gpgsign", "true");
    fx.set_config("gpg.format", "pgp5");
    fx.write("a.txt", "one\n");

    let out = ff(&fx, &["--json", "commit", "-m", "nope"]);
    assert!(!out.status.success());
    assert_eq!(json(&out)["error"]["id"], "sign/unknown-format");

    let explained = ff(&fx, &["explain", "sign/unknown-format"]);
    assert!(explained.status.success(), "stderr: {}", stderr(&explained));
    assert!(stdout(&explained).contains("gpg.format"));
}

/// The predicted sha is unknowable once signing is on — the signature is not
/// a function of anything a render has — so the `@` row carries none. The
/// column goes blank, the same one an unborn branch shows.
#[test]
fn signing_removes_the_predicted_sha_from_the_open_change() {
    let signer = good_signer();
    let fx = repo_with(&signer);
    fx.write("a.txt", "one\n");

    let before = json(&ff(&fx, &["--json", "log"]));
    assert!(
        before["data"]["open"]["pending"].is_string(),
        "an unsigned repository should still predict the close: {before}"
    );

    fx.set_config("commit.gpgsign", "true");
    let after = json(&ff(&fx, &["--json", "log"]));
    assert!(
        after["data"]["open"]["pending"].is_null(),
        "a signing repository must not claim to know the sha: {after}"
    );
}

/// `ff doctor` reports the setup without running it: off, working, and
/// broken each get their own row.
#[test]
fn doctor_reports_the_signing_setup() {
    let signer = good_signer();

    let fx = repo_with(&signer);
    assert!(stdout(&ff(&fx, &["doctor"])).contains("off (commit.gpgsign)"));

    fx.set_config("commit.gpgsign", "true");
    let on = stdout(&ff(&fx, &["doctor"]));
    assert!(on.contains("openpgp"), "{on}");

    fx.set_config("gpg.program", "definitely-not-a-signer-on-this-machine");
    let broken = stdout(&ff(&fx, &["doctor"]));
    assert!(broken.contains("is not on PATH"), "{broken}");
}

/// The default log says "signed" beside a signed commit and nothing beside an
/// unsigned one — and says it without running a signer, because carrying a
/// signature is a header on the object rather than a question for gpg.
#[test]
fn the_default_log_marks_signed_commits_and_says_nothing_about_the_rest() {
    let signer = good_signer();
    let fx = repo_with(&signer);
    fx.write("base.txt", "base\n");
    fx.commit("unsigned one");
    fx.set_config("commit.gpgsign", "true");
    fx.write("a.txt", "one\n");
    ff(&fx, &["commit", "-m", "signed one"]);

    let out = stdout(&ff(&fx, &["log"]));
    let signed_row = out
        .lines()
        .find(|line| line.contains("signed"))
        .unwrap_or_default()
        .to_string();
    assert!(
        signed_row.contains("signed"),
        "the signed commit carries no mark:\n{out}"
    );
    // The unsigned row is the one whose subject is "unsigned one"; its head
    // line must carry no mark at all.
    let lines: Vec<&str> = out.lines().collect();
    let subject_at = lines
        .iter()
        .position(|line| line.contains("unsigned one"))
        .expect("the unsigned commit is in the log");
    let head = lines[subject_at - 1];
    assert!(
        !head.contains("signed") && !head.contains("unsigned"),
        "an unsigned commit should carry no mark, got: {head:?}"
    );
}

/// The flag trades a signer run per signed row for the verdict. The stub's
/// block is not verifiable by anything, so "unverifiable" is the honest
/// answer — and it is a word, not a letter.
#[test]
fn the_signatures_flag_replaces_the_mark_with_a_verdict() {
    let signer = good_signer();
    let fx = repo_with(&signer);
    fx.set_config("commit.gpgsign", "true");
    fx.write("a.txt", "one\n");
    ff(&fx, &["commit", "-m", "signed one"]);

    let plain = stdout(&ff(&fx, &["log"]));
    assert!(plain.contains("signed"), "{plain}");

    let verified = stdout(&ff(&fx, &["log", "--signatures"]));
    assert!(
        verified.contains("unverifiable"),
        "expected a verdict word, got:\n{verified}"
    );
    // git's `%G?` letters stay on the machine surface; a row says words.
    assert!(
        !verified
            .lines()
            .any(|line| line.trim_end().ends_with(" G") || line.trim_end().ends_with(" E")),
        "a bare status letter leaked into a row:\n{verified}"
    );
}

/// `signed` rides on every log entry, so a machine reading the default log
/// learns it without asking for verification.
#[test]
fn the_machine_surface_carries_the_signed_flag() {
    let signer = good_signer();
    let fx = repo_with(&signer);
    fx.write("base.txt", "base\n");
    fx.commit("unsigned one");
    fx.set_config("commit.gpgsign", "true");
    fx.write("a.txt", "one\n");
    ff(&fx, &["commit", "-m", "signed one"]);

    let payload = json(&ff(&fx, &["--json", "log"]));
    let commits = payload["data"]["commits"].as_array().expect("commits");
    for commit in commits {
        let expected = commit["subject"] == "signed one";
        assert_eq!(
            commit["signed"], expected,
            "wrong signed flag on {}",
            commit["subject"]
        );
        // No verification was asked for, so no verdict is claimed.
        assert!(commit.get("signature").is_none());
    }
}

/// The signature `ff show` prints, and the object it puts on the machine
/// surface. The stub's block is not verifiable by anything, so the verdict is
/// `E` — which is the honest one, and still a `signature` object.
#[test]
fn show_reports_the_signature_it_finds() {
    let signer = good_signer();
    let fx = repo_with(&signer);
    fx.set_config("commit.gpgsign", "true");
    fx.write("a.txt", "one\n");
    ff(&fx, &["commit", "-m", "signed"]);

    let shown = json(&ff(&fx, &["--json", "show", "HEAD"]));
    assert_eq!(shown["data"]["signature"]["present"], true);
    assert_eq!(shown["data"]["signature"]["format"], "openpgp");

    let human = stdout(&ff(&fx, &["show", "HEAD"]));
    assert!(human.contains("signature:"), "{human}");
}

/// A signature real ssh-keygen accepts, rendered: the verdict, the tool, and
/// the eight characters that name the key — and nothing else on the row. ssh
/// rather than the stub, because only a real verifier produces a real
/// verdict; skipped where ssh-keygen is absent.
#[test]
fn a_verified_row_names_the_tool_and_the_short_key() {
    let fx = Fixture::new();
    if !fx.enable_ssh_signing() {
        eprintln!("skipping: ssh-keygen is not available");
        return;
    }
    fx.write("a.txt", "one\n");
    let out = ff(&fx, &["commit", "-m", "signed one"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    let printed = stdout(&ff(&fx, &["log", "--signatures"]));
    let head = printed
        .lines()
        .find(|line| line.contains("verified"))
        .unwrap_or_default();
    assert!(head.contains("verified ssh "), "got: {head:?}");
    // A row is a glance, not a transcript: the full fingerprint stays off it.
    assert!(
        !head.contains("SHA256:"),
        "the full fingerprint leaked: {head:?}"
    );
    let short = head.split_whitespace().last().expect("a key on the row");
    assert_eq!(short.len(), 8, "short key should be eight chars: {short:?}");

    // `ff show` is the detail view: who, plus the same short key.
    let shown = stdout(&ff(&fx, &["show", "HEAD"]));
    assert!(
        shown.contains("signature: verified — signed by committer@fixture.test (ssh "),
        "{shown}"
    );
}

/// A page of signed commits verifies in parallel and still comes back in the
/// walk's order, one status per row.
#[test]
fn a_page_of_signatures_verifies_in_order() {
    let fx = Fixture::new();
    if !fx.enable_ssh_signing() {
        eprintln!("skipping: ssh-keygen is not available");
        return;
    }
    for n in 0..6 {
        fx.write(&format!("f{n}.txt"), &format!("{n}\n"));
        let out = ff(&fx, &["commit", "-m", &format!("commit {n}")]);
        assert!(out.status.success(), "stderr: {}", stderr(&out));
    }

    let payload = json(&ff(&fx, &["--json", "log", "--signatures"]));
    let commits = payload["data"]["commits"].as_array().expect("commits");
    assert_eq!(commits.len(), 6);
    for commit in commits {
        assert_eq!(commit["signed"], true, "{}", commit["subject"]);
        assert_eq!(
            commit["signature"]["code"], "G",
            "{} did not verify: {}",
            commit["subject"], commit["signature"]["summary"]
        );
    }
    // Order is the walk's, not the order the threads happened to finish in.
    let subjects: Vec<&str> = commits
        .iter()
        .map(|c| c["subject"].as_str().unwrap())
        .collect();
    assert_eq!(
        subjects,
        vec![
            "commit 5", "commit 4", "commit 3", "commit 2", "commit 1", "commit 0"
        ]
    );
}
