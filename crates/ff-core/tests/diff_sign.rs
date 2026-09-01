//! The signing differential: commits fufu signs must be commits real git
//! verifies. Not a byte-parity suite like its siblings — a signature over a
//! commit is a function of the commit, so parity is not something two
//! independent signings could ever have — but the stronger claim underneath
//! it, that git's own verifier accepts what fufu wrote.
//!
//! ssh is the format, because it is the only one a test can use hermetically:
//! a key made in a temp directory, an allowed-signers file beside it, no
//! keyring, no agent, no passphrase. The openpgp path shares [`super`]'s
//! spawn and header mechanics with it; what differs is the program, which
//! `tests/signing.rs` covers with a stub on the CLI side.
//!
//! The whole file skips when `ssh-keygen` is absent.

use ff_core::gix;
use ff_testsupport::Fixture;

const NOW: i64 = 1_799_999_999;

/// A fixture that signs, or `None` on a machine without `ssh-keygen`.
fn signing_fixture() -> Option<Fixture> {
    let fx = Fixture::new();
    if !fx.enable_ssh_signing() {
        eprintln!("skipping: ssh-keygen is not available");
        return None;
    }
    Some(fx)
}

fn prov(verb: &str) -> ff_core::Provenance {
    ff_core::Provenance::new("pre", Some(verb.to_string()))
}

fn close(fx: &Fixture, message: &str, now: i64) -> ff_core::CommitOutcome {
    let repo = fx.repo();
    ff_core::close(
        &repo,
        &ff_core::CloseOptions {
            message: Some(message.into()),
            now: Some(now),
            argv: vec!["ff".into(), "commit".into()],
            ..Default::default()
        },
        &prov("ff commit"),
    )
    .expect("close")
    .0
}

fn oid(hex: &str) -> gix::ObjectId {
    gix::ObjectId::from_hex(hex.trim().as_bytes()).unwrap()
}

/// git's own verdict on one revision: `verify-commit` for the exit status,
/// `log --show-signature` for the words.
fn git_verifies(fx: &Fixture, rev: &str) -> bool {
    fx.try_git(&["verify-commit", rev]).status.success()
}

fn git_says_good(fx: &Fixture, rev: &str) -> bool {
    let out = fx.try_git(&["log", "--show-signature", "-1", "--format=%H", rev]);
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    said.contains("Good \"git\" signature")
}

/// The close signs, and git accepts what it wrote.
#[test]
fn a_close_writes_a_commit_git_verifies() {
    let Some(fx) = signing_fixture() else { return };
    fx.write("a.txt", "one\n");

    close(&fx, "signed", NOW);

    assert!(
        git_verifies(&fx, "HEAD"),
        "git verify-commit refused a commit fufu signed:\n{}",
        String::from_utf8_lossy(&fx.try_git(&["verify-commit", "HEAD"]).stderr)
    );
    assert!(
        git_says_good(&fx, "HEAD"),
        "git log --show-signature did not call the signature good"
    );
}

/// The header is git's header: one `gpgsig`, folded the way git folds it, and
/// the object still decodes as an ordinary commit.
#[test]
fn the_signature_lands_in_a_gpgsig_header() {
    let Some(fx) = signing_fixture() else { return };
    fx.write("a.txt", "one\n");
    close(&fx, "signed", NOW);

    let raw = fx.git(&["cat-file", "commit", "HEAD"]);
    assert_eq!(
        raw.matches("\ngpgsig ").count() + usize::from(raw.starts_with("gpgsig ")),
        1,
        "expected exactly one gpgsig header:\n{raw}"
    );
    assert!(
        raw.contains("gpgsig -----BEGIN SSH SIGNATURE-----\n ")
            && raw.contains("\n -----END SSH SIGNATURE-----\n"),
        "the armor is not folded with git's leading space on every continuation line:\n{raw}"
    );
    // The commit is otherwise ordinary: git can still read its message and
    // its tree through the same header block.
    assert_eq!(fx.git(&["log", "-1", "--format=%s"]).trim(), "signed");
}

/// A reword rewrites the commit, which kills the signature it inherited — so
/// the rewrite must mint a new one. `commit.gpgsign` governs replays too.
#[test]
fn a_reword_re_signs_the_commit_it_rewrites() {
    let Some(fx) = signing_fixture() else { return };
    fx.write("a.txt", "one\n");
    close(&fx, "first wording", NOW);
    let before = oid(&fx.git(&["rev-parse", "HEAD"]));

    let repo = fx.repo();
    ff_core::describe::reword(
        &repo,
        before,
        "second wording".to_string(),
        ff_core::Verify::Run,
        &prov("ff describe"),
        Some(NOW + 60),
        vec!["ff".into(), "describe".into()],
    )
    .expect("reword");
    drop(repo);

    let after = oid(&fx.git(&["rev-parse", "HEAD"]));
    assert_ne!(before, after, "the reword rewrote nothing");
    assert_eq!(
        fx.git(&["log", "-1", "--format=%s"]).trim(),
        "second wording"
    );
    assert!(
        git_verifies(&fx, "HEAD"),
        "the rewritten commit does not verify:\n{}",
        String::from_utf8_lossy(&fx.try_git(&["verify-commit", "HEAD"]).stderr)
    );
    assert!(git_says_good(&fx, "HEAD"));
}

/// A restack replays a whole branch. Every commit it writes is a user commit,
/// so every one of them comes back signed — the failure this feature exists
/// to prevent is exactly a restack that quietly unsigns three commits.
#[test]
fn a_restack_re_signs_every_commit_it_replays() {
    let Some(fx) = signing_fixture() else { return };

    // A trunk to move onto, then three signed commits on a branch forked
    // before trunk moved.
    fx.write("base.txt", "base\n");
    close(&fx, "base", NOW);
    let base = fx.git(&["rev-parse", "HEAD"]).trim().to_string();

    fx.git(&["checkout", "-q", "-b", "work"]);
    for (n, name) in [(1, "one"), (2, "two"), (3, "three")] {
        fx.write(&format!("{name}.txt"), name);
        close(&fx, name, NOW + n * 60);
    }

    fx.git(&["checkout", "-q", "main"]);
    fx.git(&["reset", "-q", "--hard", &base]);
    fx.write("trunk.txt", "moved\n");
    close(&fx, "trunk moves", NOW + 300);

    fx.git(&["checkout", "-q", "work"]);
    let repo = fx.repo();
    ff_core::restack::restack(
        &repo,
        Some("work".to_string()),
        Some("main".to_string()),
        &prov("ff restack"),
        Some(NOW + 360),
        vec!["ff".into(), "restack".into()],
    )
    .expect("restack");
    drop(repo);

    let replayed: Vec<String> = fx
        .git(&["rev-list", "main..work"])
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(replayed.len(), 3, "expected three replayed commits");
    for id in &replayed {
        assert!(
            git_verifies(&fx, id),
            "replayed commit {id} does not verify:\n{}",
            String::from_utf8_lossy(&fx.try_git(&["verify-commit", id]).stderr)
        );
    }
}

/// fufu's own reader agrees with git's, on the commits fufu wrote and on the
/// ones it did not.
#[test]
fn verify_reads_back_what_was_written() {
    let Some(fx) = signing_fixture() else { return };
    fx.write("a.txt", "one\n");
    close(&fx, "signed", NOW);
    let signed = oid(&fx.git(&["rev-parse", "HEAD"]));

    // An unsigned commit made by git itself, so the `N` case is a real
    // commit rather than a constructed one.
    fx.set_config("commit.gpgsign", "false");
    fx.write("b.txt", "two\n");
    let unsigned = oid(&fx.commit("plain"));

    let repo = fx.repo();
    let good = ff_core::sign::verify::verify(&repo, signed).expect("verify");
    assert!(good.present);
    assert_eq!(good.format, Some("ssh"));
    assert_eq!(good.code, 'G', "{}", good.summary);
    assert_eq!(good.signer.as_deref(), Some("committer@fixture.test"));

    let none = ff_core::sign::verify::verify(&repo, unsigned).expect("verify");
    assert!(!none.present);
    assert_eq!(none.code, 'N');
}

/// Machinery commits are not the user's work and are never signed: the op
/// journal carries fufu's own identity, and a signature on it would say
/// something untrue about who wrote it.
#[test]
fn the_operation_journal_is_not_signed() {
    let Some(fx) = signing_fixture() else { return };
    fx.write("a.txt", "one\n");
    close(&fx, "signed", NOW);

    let repo = fx.repo();
    let tip = ff_core::ops::OpLog::open(&repo)
        .unwrap()
        .tip()
        .unwrap()
        .expect("an operation")
        .object_id();
    let status = ff_core::sign::verify::verify(&repo, tip).expect("verify");
    assert!(!status.present, "an op-journal commit carries a signature");
}
