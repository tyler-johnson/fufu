//! Futures differential tests. The simulation's whole claim is that it
//! predicts a real rebase, so each test probes and then hands the same
//! question to the real `git` binary, asserting they agree — including which
//! commit a conflict lands on.
//!
//! The rebase runs last in each test, in that test's own throwaway fixture:
//! the probe has already happened and a fixture is disposable, so no scratch
//! clone is needed.

use ff_core::futures::{self, At, Verdict};
use ff_core::gix;
use ff_testsupport::Fixture;

fn oid(hex: &str) -> gix::ObjectId {
    gix::ObjectId::from_hex(hex.trim().as_bytes()).unwrap()
}

fn tip(fx: &Fixture, rev: &str) -> gix::ObjectId {
    oid(&fx.git(&["rev-parse", rev]))
}

/// Git writes the part you need to assert on to different streams depending
/// on the subcommand, so always assert against stdout and stderr together.
fn both(out: &std::process::Output) -> String {
    let mut all = String::from_utf8_lossy(&out.stdout).to_string();
    all.push_str(&String::from_utf8_lossy(&out.stderr));
    all
}

/// Three feature commits that replay cleanly onto a moved main.
fn linear_clean(fx: &Fixture) {
    fx.write("shared.txt", "line1\nline2\nline3\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    fx.write("m.txt", "main side\n");
    fx.commit("main moves");
    fx.git(&["switch", "feature"]);
    fx.write("f1.txt", "one\n");
    fx.commit("feat one");
    fx.write("f2.txt", "two\n");
    fx.commit("feat two");
    fx.write("f3.txt", "three\n");
    fx.commit("feat three");
}

/// Three feature commits whose middle one collides with main's edit.
fn conflict_in_the_middle(fx: &Fixture) {
    fx.write("shared.txt", "line1\nline2\nline3\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    fx.write("shared.txt", "MAIN\nline2\nline3\n");
    fx.commit("main edits line1");
    fx.git(&["switch", "feature"]);
    fx.write("other.txt", "ok\n");
    fx.commit("feat one clean");
    fx.write("shared.txt", "FEATURE\nline2\nline3\n");
    fx.commit("feat two conflicts");
    fx.write("third.txt", "ok\n");
    fx.commit("feat three clean");
}

#[test]
fn a_clean_verdict_means_git_rebase_succeeds() {
    let fx = Fixture::new();
    linear_clean(&fx);
    let verdict =
        futures::probe(&fx.repo(), tip(&fx, "main"), tip(&fx, "feature"), None).expect("probe");
    assert_eq!(verdict, Verdict::Clean { replayed: 3 });

    let out = fx.try_git(&["rebase", "main"]);
    assert!(
        out.status.success(),
        "git rebase should succeed: {}",
        both(&out)
    );
    assert_eq!(
        fx.git(&["rev-list", "--count", "main..feature"]).trim(),
        "3",
        "the replay count should be truthful"
    );
}

#[test]
fn a_conflict_verdict_names_the_commit_git_stops_at() {
    let fx = Fixture::new();
    conflict_in_the_middle(&fx);
    let id = tip(&fx, "feature~1").to_string();
    let verdict =
        futures::probe(&fx.repo(), tip(&fx, "main"), tip(&fx, "feature"), None).expect("probe");
    assert_eq!(
        verdict,
        Verdict::Conflict {
            at: At::Commit {
                id: id.clone(),
                subject: "feat two conflicts".to_string(),
            },
            paths: vec!["shared.txt".to_string()],
        }
    );

    let out = fx.try_git(&["rebase", "main"]);
    let all = both(&out);
    assert!(
        !out.status.success(),
        "git rebase should stop at the conflict: {all}"
    );
    assert!(
        all.contains(&id[..7]),
        "git should name the same commit it stopped at ({id}):\n{all}"
    );
    assert_eq!(
        fx.git(&["diff", "--name-only", "--diff-filter=U"]).trim(),
        "shared.txt",
        "git should name the same file"
    );

    // Leave the fixture droppable instead of mid-rebase.
    fx.try_git(&["rebase", "--abort"]);
}

#[test]
fn a_fast_forward_verdict_means_git_moves_the_pointer() {
    let fx = Fixture::new();
    fx.write("shared.txt", "line1\nline2\nline3\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    fx.write("m1.txt", "one\n");
    fx.commit("main one");
    fx.write("m2.txt", "two\n");
    fx.commit("main two");
    fx.git(&["switch", "feature"]);

    let verdict =
        futures::probe(&fx.repo(), tip(&fx, "main"), tip(&fx, "feature"), None).expect("probe");
    assert_eq!(verdict, Verdict::FastForward { behind: 2 });

    let out = fx.try_git(&["rebase", "main"]);
    assert!(
        out.status.success(),
        "git rebase should succeed: {}",
        both(&out)
    );
    assert_eq!(
        fx.git(&["rev-parse", "feature"]).trim(),
        fx.git(&["rev-parse", "main"]).trim(),
        "a fast-forward is a pointer move, not new commits"
    );
}

#[test]
fn an_up_to_date_verdict_means_git_does_nothing() {
    let fx = Fixture::new();
    fx.write("shared.txt", "line1\nline2\nline3\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    fx.git(&["switch", "feature"]);
    fx.write("f1.txt", "one\n");
    fx.commit("feat one");
    fx.write("f2.txt", "two\n");
    fx.commit("feat two");

    let before = fx.git(&["rev-parse", "feature"]).trim().to_string();
    let verdict =
        futures::probe(&fx.repo(), tip(&fx, "main"), tip(&fx, "feature"), None).expect("probe");
    assert_eq!(verdict, Verdict::UpToDate { ahead: 2 });

    let out = fx.try_git(&["rebase", "main"]);
    assert!(
        out.status.success(),
        "git rebase should succeed: {}",
        both(&out)
    );
    assert_eq!(
        fx.git(&["rev-parse", "feature"]).trim(),
        before,
        "an up-to-date rebase must not move the branch"
    );
}

#[test]
fn the_open_change_verdict_matches_a_stash_backed_rebase() {
    let fx = Fixture::new();
    fx.write("shared.txt", "line1\nline2\nline3\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    fx.write("shared.txt", "MAIN\nline2\nline3\n");
    fx.commit("main edits line1");
    fx.git(&["switch", "feature"]);
    fx.write("other.txt", "ok\n");
    fx.commit("feat one clean");
    // Conflicting work that was never committed.
    fx.write("shared.txt", "FEATURE\nline2\nline3\n");
    fx.git(&["add", "-A"]);
    let open = oid(&fx.git(&["write-tree"]));

    let verdict = futures::probe(
        &fx.repo(),
        tip(&fx, "main"),
        tip(&fx, "feature"),
        Some(open),
    )
    .expect("probe");
    assert_eq!(
        verdict,
        Verdict::Conflict {
            at: At::OpenChange,
            paths: vec!["shared.txt".to_string()],
        }
    );

    // The same conflict, the way a person would hit it. Git exits 0 even
    // though the autostash pop conflicted — the failure of the reapply does
    // not reach the exit code — so only the message and the tree state are
    // asserted.
    //
    // Git spells this two ways depending on its version: "Applying autostash
    // resulted in conflicts." on 2.50 and 2.54, and a longer "Your local
    // changes are stashed, however applying them / resulted in conflicts."
    // on what the macOS and Windows CI runners carry. Matching the phrase
    // both share keeps this about git's behavior rather than git's prose —
    // and the behavior is the same either way: the conflict is left in the
    // working tree, which is what the assertion below is really for.
    let out = fx.try_git(&["rebase", "--autostash", "main"]);
    let all = both(&out);
    assert!(
        all.contains("resulted in conflicts"),
        "git should report the autostash conflict:\n{all}"
    );
    let status = fx.git(&["status", "--porcelain=v2"]);
    assert!(
        status
            .lines()
            .any(|l| l.starts_with("u ") && l.ends_with(" shared.txt")),
        "the working tree should hold the conflict the verdict predicted:\n{status}"
    );
}

#[test]
fn git_and_the_simulation_agree_on_a_clean_three_commit_replay_tree() {
    let fx = Fixture::new();
    linear_clean(&fx);
    let verdict =
        futures::probe(&fx.repo(), tip(&fx, "main"), tip(&fx, "feature"), None).expect("probe");
    assert_eq!(verdict, Verdict::Clean { replayed: 3 });

    let out = fx.try_git(&["rebase", "main"]);
    assert!(
        out.status.success(),
        "git rebase should succeed: {}",
        both(&out)
    );

    let tree = fx.git(&["ls-tree", "--name-only", "feature"]);
    for file in ["f1.txt", "f2.txt", "f3.txt", "m.txt", "shared.txt"] {
        assert!(
            tree.lines().any(|line| line.trim() == file),
            "a clean replay keeps both sides' work, missing {file}:\n{tree}"
        );
    }
}
