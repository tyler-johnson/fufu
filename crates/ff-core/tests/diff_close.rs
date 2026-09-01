//! Differential contract for the close: `ff commit` must produce the same
//! tree as `git add -A && git commit`, the same parent topology and message,
//! and leave the index in the same state (semantically — fufu indexes carry
//! their own extension shape). Hooks behave like git's; the pending
//! description is consumed; `-b` forks and claims.

use ff_core::{CloseOptions, CommitOutcome, Result};
use ff_testsupport::hooks::{STAGED_HOOK, install_hook, staged_marker};
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
    close_result(fx, opts).unwrap()
}

/// The same call without the unwrap, for scenarios that may refuse.
fn close_result(
    fx: &Fixture,
    opts: CloseOptions,
) -> Result<(CommitOutcome, ff_core::ops::VerbContext)> {
    let repo = fx.repo();
    ff_core::close(
        &repo,
        &opts,
        &ff_core::Provenance::new("pre", Some("ff commit".into())),
    )
}

fn default_opts() -> CloseOptions {
    CloseOptions {
        message: Some("close message".into()),
        verify: Default::default(),
        branch: None,
        sign: Default::default(),
        paths: Vec::new(),
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

        let closed = close_result(&fx_ff, default_opts());

        fx_git.git(&["add", "-A"]);
        let git_commit = fx_git.try_git(&["commit", "-q", "-m", "close message"]);

        match closed {
            Err(err) => {
                assert_eq!(
                    err.id(),
                    "commit/empty",
                    "scenario {name}: the refusal is the clean-tree one"
                );
                assert!(
                    !git_commit.status.success(),
                    "scenario {name}: fufu refuses where git commits"
                );
            }
            Ok((CommitOutcome::Closed { id, .. }, _ctx)) => {
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
            parent: None,
            session: None,
            held: None,
            resolving: None,
        },
    )
    .unwrap();

    fx.write("a.txt", "changed\n");
    let mut opts = default_opts();
    opts.message = None; // no -m: the pending description is the message
    let (outcome, _) = close_with(&fx, opts);
    let CommitOutcome::Closed { id, subject, .. } = outcome;
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
            parent: None,
            session: None,
            held: None,
            resolving: None,
        },
    )
    .unwrap();
    fx.write("a.txt", "changed\n");
    let (outcome, _) = close_with(&fx, default_opts());
    let CommitOutcome::Closed { subject, .. } = outcome;
    assert_eq!(subject, "close message");
    let meta = ff_core::branchmeta::read(&fx.repo(), "main").unwrap();
    assert!(meta.pending_description.is_none(), "still consumed");
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
    let CommitOutcome::Closed { id, .. } = outcome;
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
    // The close populates the index before the hook; a decline must put it
    // back exactly, git's own rollback.
    let index_before = fx.index_bytes();

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
    assert_eq!(
        fx.index_bytes(),
        index_before,
        "the declined close restores .git/index byte-for-byte"
    );
    // No op entry was journaled for the aborted close.
    let ops = ff_core::ops::read_ops(&fx.repo(), 0).unwrap();
    assert!(ops.iter().all(|op| op.verb != "commit"), "{ops:?}");

    // --no-verify skips the hook entirely.
    let mut opts = default_opts();
    opts.verify = ff_core::Verify::Skip;
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
    let CommitOutcome::Closed { subject, .. } = outcome;
    assert_eq!(subject, "rewritten: close message");
}

#[test]
fn pre_commit_hook_sees_the_whole_change_staged() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    install_hook(&fx, "pre-commit", STAGED_HOOK);
    fx.write("a.txt", "changed\n");
    fx.write("new.txt", "new\n");

    let (outcome, _) = close_with(&fx, default_opts());
    assert!(matches!(outcome, CommitOutcome::Closed { .. }));
    assert_eq!(
        staged_marker(&fx),
        vec!["a.txt", "new.txt"],
        "hook-runners keyed on the index see the whole change"
    );
}

#[test]
fn pre_commit_hook_sees_only_the_selected_paths() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.write("b.txt", "b\n");
    fx.commit("init");
    ident(&fx);
    install_hook(&fx, "pre-commit", STAGED_HOOK);
    fx.write("a.txt", "a changed\n");
    fx.write("b.txt", "b changed\n");

    let mut opts = default_opts();
    opts.paths = vec!["a.txt".into()];
    let (outcome, _) = close_with(&fx, opts);
    assert!(matches!(outcome, CommitOutcome::Closed { .. }));
    assert_eq!(
        staged_marker(&fx),
        vec!["a.txt"],
        "a partial close stages exactly the slice, as git's pathspec form does"
    );
    // The remainder is still the open change, on disk and unstaged.
    assert_eq!(
        std::fs::read_to_string(fx.path().join("b.txt")).unwrap(),
        "b changed\n",
        "the unselected edit survives the hook run"
    );
}

#[test]
fn commit_msg_hook_sees_the_change_staged() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    // No pre-commit hook: the gate's second arm carries this alone.
    install_hook(&fx, "commit-msg", STAGED_HOOK);
    fx.write("a.txt", "changed\n");

    let (outcome, _) = close_with(&fx, default_opts());
    assert!(matches!(outcome, CommitOutcome::Closed { .. }));
    assert_eq!(staged_marker(&fx), vec!["a.txt"]);
}

#[test]
fn an_inert_pre_commit_hook_still_closes_the_whole_change() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    install_hook(&fx, "pre-commit", "#!/bin/sh\nexit 0\n");
    fx.write("a.txt", "changed\n");
    fx.write("new.txt", "new\n");

    // The provisional index must never be HEAD's tree, or the capture scan's
    // staged-known-clean shortcut fires and a real change refuses as empty.
    let (outcome, _) = close_with(&fx, default_opts());
    let CommitOutcome::Closed { id, .. } = outcome;
    assert_eq!(fx.git(&["show", &format!("{id}:a.txt")]), "changed\n");
    assert_eq!(fx.git(&["show", &format!("{id}:new.txt")]), "new\n");
}

#[test]
fn a_hook_reverting_the_worktree_refuses_and_restores_the_index() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let head = fx.commit("init");
    ident(&fx);
    install_hook(&fx, "pre-commit", "#!/bin/sh\nprintf 'a\\n' > a.txt\n");
    fx.write("a.txt", "changed\n");
    let index_before = fx.index_bytes();

    let err = close_result(&fx, default_opts()).unwrap_err();
    assert_eq!(
        err.id(),
        "commit/empty",
        "the re-scan finds nothing to close"
    );
    assert_eq!(
        fx.git(&["rev-parse", "HEAD"]).trim(),
        head,
        "branch unmoved"
    );
    assert_eq!(
        fx.index_bytes(),
        index_before,
        "a close that does not land restores .git/index byte-for-byte"
    );
}

#[test]
fn prepare_commit_msg_receives_the_file_and_its_source() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    // Record the arguments, then edit the message in place — the two things
    // git's contract for this hook is made of.
    install_hook(
        &fx,
        "prepare-commit-msg",
        "#!/bin/sh\nprintf '%s\\n' \"$2\" > \"$(git rev-parse --git-dir)/source.txt\"\n\
         printf 'prepared: ' > \"$1.tmp\"\ncat \"$1\" >> \"$1.tmp\"\nmv \"$1.tmp\" \"$1\"\n",
    );
    fx.write("a.txt", "changed\n");

    let (outcome, _) = close_with(&fx, default_opts());
    let CommitOutcome::Closed { subject, .. } = outcome;
    assert_eq!(
        subject, "prepared: close message",
        "the hook's in-place edit is what gets committed"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join(".git/source.txt")).unwrap(),
        "message\n",
        "-m is git's `message` source"
    );
}

#[test]
fn both_message_hooks_edit_one_file_in_order() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    install_hook(
        &fx,
        "prepare-commit-msg",
        "#!/bin/sh\nprintf 'one: ' > \"$1.tmp\"\ncat \"$1\" >> \"$1.tmp\"\nmv \"$1.tmp\" \"$1\"\n",
    );
    install_hook(
        &fx,
        "commit-msg",
        "#!/bin/sh\nprintf 'two: ' > \"$1.tmp\"\ncat \"$1\" >> \"$1.tmp\"\nmv \"$1.tmp\" \"$1\"\n",
    );
    fx.write("a.txt", "changed\n");

    let (outcome, _) = close_with(&fx, default_opts());
    let CommitOutcome::Closed { subject, .. } = outcome;
    assert_eq!(
        subject, "two: one: close message",
        "prepare-commit-msg runs first and commit-msg sees its result"
    );
}

#[test]
fn no_verify_keeps_prepare_commit_msg_and_drops_the_other_two() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    // pre-commit and commit-msg would both refuse the close outright.
    install_hook(&fx, "pre-commit", "#!/bin/sh\nexit 1\n");
    install_hook(&fx, "commit-msg", "#!/bin/sh\nexit 1\n");
    install_hook(
        &fx,
        "prepare-commit-msg",
        "#!/bin/sh\nprintf 'prepared: ' > \"$1.tmp\"\ncat \"$1\" >> \"$1.tmp\"\nmv \"$1.tmp\" \"$1\"\n",
    );
    fx.write("a.txt", "changed\n");

    let mut opts = default_opts();
    opts.verify = ff_core::Verify::Skip;
    let (outcome, _) = close_with(&fx, opts);
    let CommitOutcome::Closed { subject, .. } = outcome;
    assert_eq!(
        subject, "prepared: close message",
        "githooks(5): --no-verify does not suppress prepare-commit-msg"
    );
}

#[test]
fn declining_prepare_commit_msg_aborts_with_nothing_written() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let head = fx.commit("init");
    ident(&fx);
    install_hook(&fx, "prepare-commit-msg", "#!/bin/sh\nexit 1\n");
    fx.write("a.txt", "changed\n");
    let index_before = fx.index_bytes();

    let err = close_result(&fx, default_opts()).unwrap_err();
    assert_eq!(err.id(), "hook/declined");
    assert_eq!(
        fx.git(&["rev-parse", "HEAD"]).trim(),
        head,
        "branch unmoved"
    );
    assert_eq!(
        fx.index_bytes(),
        index_before,
        "the declined close restores .git/index byte-for-byte"
    );
    let ops = ff_core::ops::read_ops(&fx.repo(), 0).unwrap();
    assert!(ops.iter().all(|op| op.verb != "commit"), "{ops:?}");
}

#[test]
fn post_commit_runs_after_the_commit_exists() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    // Non-zero on purpose: post-commit cannot fail the close, and the hook
    // must still see the landed HEAD before it exits.
    install_hook(
        &fx,
        "post-commit",
        "#!/bin/sh\ngit rev-parse HEAD > \"$(git rev-parse --git-dir)/landed.txt\"\nexit 3\n",
    );
    fx.write("a.txt", "changed\n");

    let (outcome, _) = close_with(&fx, default_opts());
    let CommitOutcome::Closed { id, .. } = outcome;
    assert_eq!(
        std::fs::read_to_string(fx.path().join(".git/landed.txt"))
            .unwrap()
            .trim(),
        id,
        "post-commit sees the commit it is notifying about"
    );
}

#[test]
fn commit_hooks_see_git_editor_disabled() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    let record = |name: &str| {
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$GIT_EDITOR\" >> \"$(git rev-parse --git-dir)/editor-{name}.txt\"\nexit 0\n"
        )
    };
    for name in [
        "pre-commit",
        "prepare-commit-msg",
        "commit-msg",
        "post-commit",
    ] {
        install_hook(&fx, name, &record(name));
    }
    fx.write("a.txt", "changed\n");

    let (outcome, _) = close_with(&fx, default_opts());
    assert!(matches!(outcome, CommitOutcome::Closed { .. }));
    for name in [
        "pre-commit",
        "prepare-commit-msg",
        "commit-msg",
        "post-commit",
    ] {
        assert_eq!(
            std::fs::read_to_string(fx.path().join(format!(".git/editor-{name}.txt")))
                .unwrap_or_else(|_| panic!("{name} ran")),
            ":\n",
            "{name} runs with GIT_EDITOR=:"
        );
    }
}

#[test]
fn no_verify_writes_no_provisional_index() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    fx.git(&["branch", "taken"]);
    install_hook(&fx, "pre-commit", STAGED_HOOK);
    fx.write("a.txt", "changed\n");
    let index_before = fx.index_bytes();

    // Refuses at the branch axis, well past where the provisional write
    // would have happened.
    let mut opts = default_opts();
    opts.verify = ff_core::Verify::Skip;
    opts.branch = Some("taken".into());
    let err = close_result(&fx, opts).unwrap_err();
    assert_eq!(err.id(), "branch/exists");
    assert!(
        !fx.path().join(".git/staged.txt").exists(),
        "--no-verify runs no hook"
    );
    assert_eq!(
        fx.index_bytes(),
        index_before,
        "--no-verify touches the index only at the final write"
    );
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
    let CommitOutcome::Closed { branch, id, .. } = outcome;
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
    } = outcome;
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
    let CommitOutcome::Closed { id, .. } = outcome;

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
    let CommitOutcome::Closed { id, branch, .. } = outcome;
    assert_eq!(branch, "main");
    let parents = fx.git(&["log", "--format=%P", "-1", &id]);
    assert_eq!(parents.trim(), "", "initial commit has no parents");
    assert_eq!(fx.git(&["rev-parse", "HEAD"]).trim(), id);
    assert_eq!(fx.git(&["status", "--porcelain=v2"]), "");
}
