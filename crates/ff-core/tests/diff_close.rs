//! Differential contract for the close: `ff commit` must produce the same
//! tree as `git add -A && git commit`, the same parent topology and message,
//! and leave the index in the same state (semantically — fufu indexes carry
//! their own extension shape). Hooks behave like git's; the pending
//! description is consumed; `-b` forks and claims.

use ff_core::{CloseOptions, CommitOutcome};
use ff_testsupport::{Fixture, scenarios};

/// The newest operation's record, read through the public reader.
fn tip_record(repo: &gix::Repository) -> ff_core::ops::OpRecord {
    let log = ff_core::ops::OpLog::open(repo).unwrap();
    let op = log.get(log.tip().unwrap().unwrap()).unwrap();
    op.record()
        .unwrap()
        .cloned()
        .expect("a verb op has a record")
}

const NOW: i64 = 1_700_000_000;

fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Close User");
    fx.set_config("user.email", "close@test");
}

fn close_with(fx: &Fixture, opts: CloseOptions) -> (CommitOutcome, ff_core::ops::VerbContext) {
    let repo = fx.repo();
    ff_core::close(
        &repo,
        &opts,
        &ff_core::Provenance::new("pre", Some("ff commit".into())),
    )
    .unwrap()
}

fn default_opts() -> CloseOptions {
    CloseOptions {
        message: Some("close message".into()),
        no_verify: false,
        branch: None,
        now: Some(NOW),
        argv: vec!["ff".into(), "commit".into()],
    }
}

#[test]
fn matrix_close_matches_git_add_a_commit() {
    for (name, setup) in scenarios() {
        let fx_ff = Fixture::new();
        setup(&fx_ff);
        ident(&fx_ff);
        let fx_git = Fixture::new();
        setup(&fx_git);
        ident(&fx_git);

        let repo = fx_ff.repo();
        let head = ff_core::head_state(&repo).unwrap();
        let in_operation = ff_core::operation(&repo).is_some();
        if in_operation {
            // Mid-merge closes refuse, pointing at git.
            let err = ff_core::close(
                &repo,
                &default_opts(),
                &ff_core::Provenance::new("pre", None),
            );
            assert!(
                err.is_err(),
                "scenario {name}: close must refuse mid-operation"
            );
            continue;
        }
        if matches!(head, ff_core::HeadState::Detached { .. }) {
            let err = ff_core::close(
                &repo,
                &default_opts(),
                &ff_core::Provenance::new("pre", None),
            );
            assert!(err.is_err(), "scenario {name}: close refuses detached HEAD");
            continue;
        }

        let (outcome, _ctx) = close_with(&fx_ff, default_opts());

        fx_git.git(&["add", "-A"]);
        let git_commit = fx_git.try_git(&["commit", "-q", "--allow-empty", "-m", "close message"]);

        match outcome {
            CommitOutcome::NothingToClose { .. } => {
                panic!("scenario {name}: close with -m never no-ops under the message-aware rule")
            }
            CommitOutcome::Closed { id, .. } => {
                assert!(
                    git_commit.status.success(),
                    "scenario {name}: close commits where git refuses"
                );
                // Trees are content-addressed: equality across fixtures is
                // byte-equality of committed state.
                let f_tree = fx_ff.git(&["rev-parse", &format!("{id}^{{tree}}")]);
                let g_tree = fx_git.git(&["rev-parse", "HEAD^{tree}"]);
                assert_eq!(f_tree, g_tree, "scenario {name}: committed tree");
                let f_parents = fx_ff.git(&["log", "--format=%P", "-1", &id]);
                let g_parents = fx_git.git(&["log", "--format=%P", "-1", "HEAD"]);
                assert_eq!(
                    f_parents.split_whitespace().count(),
                    g_parents.split_whitespace().count(),
                    "scenario {name}: parent count"
                );
                let f_msg = fx_ff.git(&["log", "--format=%B", "-1", &id]);
                let g_msg = fx_git.git(&["log", "--format=%B", "-1", "HEAD"]);
                assert_eq!(f_msg, g_msg, "scenario {name}: message");
                // Post-close: index equals the tree, worktree reads clean of
                // staged changes, exactly like git's.
                assert_eq!(
                    fx_ff.git(&["ls-files", "--stage"]),
                    fx_git.git(&["ls-files", "--stage"]),
                    "scenario {name}: post-close index"
                );
                assert_eq!(
                    fx_ff.git(&["status", "--porcelain=v2"]),
                    fx_git.git(&["status", "--porcelain=v2"]),
                    "scenario {name}: post-close status"
                );
                // And git accepts the world we left behind.
                assert_eq!(fx_ff.git(&["write-tree"]).trim(), f_tree.trim());
            }
        }
    }
}

#[test]
fn pending_description_is_consumed_by_the_close() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    let repo = fx.repo();
    ff_core::branchmeta::write(
        &repo,
        "main",
        &ff_core::branchmeta::BranchMeta {
            pending_description: Some("planned: the pending text".into()),
            forked_from: None,
        },
    )
    .unwrap();

    fx.write("a.txt", "changed\n");
    let mut opts = default_opts();
    opts.message = None; // no -m: the pending description is the message
    let (outcome, _) = close_with(&fx, opts);
    let CommitOutcome::Closed { id, subject, .. } = outcome else {
        panic!("expected a close");
    };
    assert_eq!(subject, "planned: the pending text");
    let msg = fx.git(&["log", "--format=%B", "-1", &id]);
    assert_eq!(msg.trim_end(), "planned: the pending text");
    let meta = ff_core::branchmeta::read(&fx.repo(), "main").unwrap();
    assert!(meta.pending_description.is_none(), "consumed");
}

#[test]
fn dash_m_wins_over_pending_and_still_consumes_it() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    let repo = fx.repo();
    ff_core::branchmeta::write(
        &repo,
        "main",
        &ff_core::branchmeta::BranchMeta {
            pending_description: Some("stale pending".into()),
            forked_from: None,
        },
    )
    .unwrap();
    fx.write("a.txt", "changed\n");
    let (outcome, _) = close_with(&fx, default_opts());
    let CommitOutcome::Closed { subject, .. } = outcome else {
        panic!("expected a close");
    };
    assert_eq!(subject, "close message");
    let meta = ff_core::branchmeta::read(&fx.repo(), "main").unwrap();
    assert!(meta.pending_description.is_none(), "still consumed");
}

fn install_hook(fx: &Fixture, name: &str, body: &str) {
    let dir = fx.path().join(".git/hooks");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    // Windows has no exec bit; hook discovery there is existence-only,
    // and the `#!/bin/sh` body runs via Git Bash's sh, as under git itself.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn pre_commit_hook_changes_are_included_by_the_rescan() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    install_hook(
        &fx,
        "pre-commit",
        "#!/bin/sh\nprintf 'formatted\\n' > a.txt\n",
    );
    fx.write("a.txt", "unformatted\n");
    let (outcome, _) = close_with(&fx, default_opts());
    let CommitOutcome::Closed { id, .. } = outcome else {
        panic!("expected a close");
    };
    let content = fx.git(&["show", &format!("{id}:a.txt")]);
    assert_eq!(content, "formatted\n", "the hook's formatting is committed");
}

#[test]
fn declining_pre_commit_hook_aborts_with_nothing_written() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let head = fx.commit("init");
    ident(&fx);
    install_hook(&fx, "pre-commit", "#!/bin/sh\nexit 1\n");
    fx.write("a.txt", "changed\n");

    let repo = fx.repo();
    let err = ff_core::close(
        &repo,
        &default_opts(),
        &ff_core::Provenance::new("pre", None),
    );
    assert!(err.is_err(), "hook declined");
    assert_eq!(
        fx.git(&["rev-parse", "HEAD"]).trim(),
        head,
        "branch unmoved"
    );
    // No op entry was journaled for the aborted close.
    let ops = ff_core::ops::read_ops(&fx.repo(), 0).unwrap();
    assert!(ops.iter().all(|op| op.verb != "commit"), "{ops:?}");

    // --no-verify skips the hook entirely.
    let mut opts = default_opts();
    opts.no_verify = true;
    let (outcome, _) = close_with(&fx, opts);
    assert!(matches!(outcome, CommitOutcome::Closed { .. }));
}

#[test]
fn commit_msg_hook_rewrites_the_message() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    install_hook(
        &fx,
        "commit-msg",
        "#!/bin/sh\nprintf 'rewritten: ' > \"$1.tmp\"\ncat \"$1\" >> \"$1.tmp\"\nmv \"$1.tmp\" \"$1\"\n",
    );
    fx.write("a.txt", "changed\n");
    let (outcome, _) = close_with(&fx, default_opts());
    let CommitOutcome::Closed { subject, .. } = outcome else {
        panic!("expected a close");
    };
    assert_eq!(subject, "rewritten: close message");
}

#[test]
fn dash_b_fresh_name_forks_and_the_old_branch_stays() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let base = fx.commit("init");
    ident(&fx);
    fx.write("a.txt", "feature work\n");
    let mut opts = default_opts();
    opts.branch = Some("feature".into());
    let (outcome, _) = close_with(&fx, opts);
    let CommitOutcome::Closed { branch, id, .. } = outcome else {
        panic!("expected a close");
    };
    assert_eq!(branch, "feature");
    assert_eq!(
        fx.git(&["rev-parse", "refs/heads/main"]).trim(),
        base,
        "main stays"
    );
    assert_eq!(fx.git(&["rev-parse", "refs/heads/feature"]).trim(), id);
    assert_eq!(
        fx.git(&["symbolic-ref", "HEAD"]).trim(),
        "refs/heads/feature",
        "HEAD followed the close"
    );
}

#[test]
fn dash_b_claims_an_anonymous_branch_carrying_its_chain() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    // An anonymous branch with an existing snap chain.
    fx.git(&["checkout", "-q", "-b", "ff/quick-fox"]);
    fx.write("a.txt", "anon work\n");
    let repo = fx.repo();
    ff_core::capture(&repo, &ff_core::Provenance::new("manual", None)).unwrap();
    let chain_tip = fx
        .git(&["rev-parse", "refs/fufu/snap/ff/quick-fox"])
        .trim()
        .to_string();

    let mut opts = default_opts();
    opts.branch = Some("real-name".into());
    let (outcome, _) = close_with(&fx, opts);
    let CommitOutcome::Closed {
        branch,
        claimed_from,
        ..
    } = outcome
    else {
        panic!("expected a close");
    };
    assert_eq!(branch, "real-name");
    assert_eq!(claimed_from.as_deref(), Some("ff/quick-fox"));
    // The old name is gone; the chain followed the claim.
    assert!(
        !fx.try_git(&["rev-parse", "--verify", "refs/heads/ff/quick-fox"])
            .status
            .success()
    );
    let moved = fx.git(&["rev-parse", "refs/fufu/snap/real-name"]);
    // The pre-verb snapshot advanced the chain past our manual tip; the
    // manual tip must still be an ancestor on the renamed chain.
    let merge_base = fx.git(&["merge-base", moved.trim(), &chain_tip]);
    assert_eq!(merge_base.trim(), chain_tip, "chain history carried");
    assert_eq!(
        fx.git(&["symbolic-ref", "HEAD"]).trim(),
        "refs/heads/real-name"
    );
}

#[test]
fn close_journals_one_op_entry_and_reconciles_clean_after() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    fx.write("a.txt", "changed\n");
    let (outcome, _) = close_with(&fx, default_opts());
    let CommitOutcome::Closed { id, .. } = outcome else {
        panic!("expected a close");
    };

    let repo = fx.repo();
    let record = tip_record(&repo);
    assert_eq!(record.verb, "commit");

    let main = record
        .refs
        .iter()
        .find(|t| t.name == "refs/heads/main")
        .expect("main transition journaled");
    assert_eq!(main.new.as_deref(), Some(id.as_str()));

    // The mutation matched the plan: the next pass is clean.
    let report = ff_core::ops::reconcile(&repo, NOW + 10).unwrap();
    assert!(report.is_quiet(), "{report:?}");
}

#[test]
fn unborn_close_writes_the_initial_commit() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("first.txt", "first\n");
    let (outcome, _) = close_with(&fx, default_opts());
    let CommitOutcome::Closed { id, branch, .. } = outcome else {
        panic!("expected a close");
    };
    assert_eq!(branch, "main");
    let parents = fx.git(&["log", "--format=%P", "-1", &id]);
    assert_eq!(parents.trim(), "", "initial commit has no parents");
    assert_eq!(fx.git(&["rev-parse", "HEAD"]).trim(), id);
    assert_eq!(fx.git(&["status", "--porcelain=v2"]), "");
}
