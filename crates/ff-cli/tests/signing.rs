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
