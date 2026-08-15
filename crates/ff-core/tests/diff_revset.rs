//! The revset back end against the real `git` binary.
//!
//! "gitrevisions entire" is a promise, and this is what makes it a checked
//! claim rather than a wish: every suffix form, every abbreviation length,
//! tags, remote refs and the ranges are asserted against `git rev-parse` for
//! points and `git rev-list` for sets. Where fufu deliberately differs — `@`,
//! `a...b`, `x^!`, the canonical-base invariant — the test asserts *fufu's*
//! behavior and says why, so a divergence can only ever be a decision
//! somebody wrote down.

use ff_core::revset::{Rev, Revset};
use ff_testsupport::Fixture;

/// A history with every shape the suffixes need: a merge for `^2`, two kinds
/// of tag, a remote-tracking ref, and an upstream to navigate to.
struct World {
    fx: Fixture,
    c1: String,
    c2: String,
    c3: String,
    c5: String,
    merge: String,
}

fn world() -> World {
    let fx = Fixture::new();
    fx.write("a.txt", "1\n");
    let c1 = fx.commit("one");
    fx.write("a.txt", "2\n");
    let c2 = fx.commit("two");
    fx.write("a.txt", "3\n");
    let c3 = fx.commit("three");

    fx.git(&["checkout", "-q", "-b", "feature"]);
    fx.write("b.txt", "4\n");
    let _c4 = fx.commit("four on the feature");
    fx.git(&["checkout", "-q", "main"]);
    fx.write("a.txt", "5\n");
    let c5 = fx.commit("five");
    fx.git(&["merge", "-q", "--no-ff", "-m", "merge feature", "feature"]);
    let merge = fx.git(&["rev-parse", "HEAD"]).trim().to_string();

    fx.git(&["tag", "v1", &c2]);
    fx.git(&["tag", "-a", "-m", "release two", "v2", &c3]);
    fx.git(&["update-ref", "refs/remotes/origin/main", &c5]);
    fx.set_config("remote.origin.url", "https://example.invalid/repo.git");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");
    fx.set_config("branch.feature.remote", "origin");
    fx.set_config("branch.feature.merge", "refs/heads/main");

    World {
        fx,
        c1,
        c2,
        c3,
        c5,
        merge,
    }
}

// --- fufu's side ---

fn point(fx: &Fixture, src: &str) -> String {
    let repo = fx.repo();
    let p = Revset::parse(src)
        .unwrap_or_else(|e| panic!("{src} parses: {e}"))
        .point(&repo)
        .unwrap_or_else(|e| panic!("{src} resolves: {e}"));
    match p.rev {
        Rev::Commit(id) => id.to_string(),
        Rev::Open => panic!("{src} resolved to the open change"),
    }
}

fn point_name(fx: &Fixture, src: &str) -> Option<String> {
    let repo = fx.repo();
    Revset::parse(src)
        .expect("parses")
        .point(&repo)
        .expect("resolves")
        .name
}

fn set(fx: &Fixture, src: &str) -> Vec<String> {
    let repo = fx.repo();
    Revset::parse(src)
        .unwrap_or_else(|e| panic!("{src} parses: {e}"))
        .evaluate(&repo)
        .unwrap_or_else(|e| panic!("{src} binds: {e}"))
        .map(|rev| match rev.expect("member") {
            Rev::Commit(id) => id.to_string(),
            Rev::Open => "@".to_string(),
        })
        .collect()
}

/// The id of the refusal `src` raises, whether at parse or at bind time.
fn refusal(fx: &Fixture, src: &str) -> String {
    let repo = fx.repo();
    let parsed = match Revset::parse(src) {
        Err(err) => return err.id().to_string(),
        Ok(parsed) => parsed,
    };
    match parsed.evaluate(&repo) {
        Err(err) => err.id().to_string(),
        Ok(mut iter) => match iter.next() {
            Some(Err(err)) => err.id().to_string(),
            _ => panic!("{src} was not refused"),
        },
    }
}

// --- git's side ---

fn git_point(fx: &Fixture, spec: &str) -> String {
    fx.git(&["rev-parse", &format!("{spec}^{{commit}}")])
        .trim()
        .to_string()
}

fn git_set(fx: &Fixture, args: &[&str]) -> Vec<String> {
    let mut argv = vec!["rev-list"];
    argv.extend_from_slice(args);
    fx.git(&argv)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

// --- points: every suffix form gitrevisions has ---

#[test]
fn every_suffix_form_agrees_with_rev_parse() {
    let w = world();
    for spec in [
        "main",
        "main^",
        "main^1",
        "main^2",
        "main~",
        "main~3",
        "main^^",
        "main~2^",
        "main^{}",
        "main^{commit}",
        "refs/heads/main",
        "feature",
        "v1",
        "v2",
        "refs/tags/v1",
        "refs/tags/v2",
        "origin/main",
        "refs/remotes/origin/main",
        "HEAD",
        "HEAD~1",
        "HEAD^",
    ] {
        assert_eq!(
            point(&w.fx, spec),
            git_point(&w.fx, spec),
            "{spec} must resolve where git resolves it"
        );
    }
}

#[test]
fn abbreviations_of_every_length_agree() {
    let w = world();
    // Four is git's own floor; below it a hex string is an ordinary name.
    for len in [4usize, 7, 10, 20, 40] {
        let prefix = &w.c3[..len];
        assert_eq!(
            point(&w.fx, prefix),
            w.c3,
            "a {len}-character abbreviation must resolve"
        );
    }
    // Three characters is not an abbreviation git would take, so it falls
    // through to the name lookup and finds nothing.
    assert_eq!(
        refusal(&w.fx, &w.c3[..3]),
        "usage/revset-unknown-revision",
        "below git's floor a hex string is a name, not an id"
    );
}

#[test]
fn reflog_and_upstream_navigate_from_a_ref() {
    let w = world();
    // The canonical base is a ref *name*, not the sha it holds, which is the
    // whole reason these two still have somewhere to navigate from.
    assert_eq!(point(&w.fx, "main@{0}"), git_point(&w.fx, "main@{0}"));
    assert_eq!(point(&w.fx, "main@{1}"), git_point(&w.fx, "main@{1}"));
    assert_eq!(
        point(&w.fx, "feature@{upstream}"),
        git_point(&w.fx, "feature@{upstream}")
    );
    assert_eq!(
        point(&w.fx, "feature@{u}"),
        git_point(&w.fx, "feature@{upstream}")
    );
    // A bare `@{…}` is gitrevisions, not fufu's `@`: `@{` cannot open a ref
    // name, so the two can never be confused.
    assert_eq!(point(&w.fx, "@{1}"), git_point(&w.fx, "@{1}"));
}

#[test]
fn message_search_suffix_agrees() {
    let w = world();
    assert_eq!(point(&w.fx, "main^{/two}"), git_point(&w.fx, "main^{/two}"));
    assert_eq!(point(&w.fx, "main^{/two}"), w.c2);
}

#[test]
fn a_branch_leaf_reports_its_name_and_anything_else_does_not() {
    let w = world();
    // The one caller this exists for: `start` reports `forked_from` as a
    // branch name when the reader named a branch, and a short sha otherwise.
    assert_eq!(point_name(&w.fx, "main").as_deref(), Some("main"));
    assert_eq!(
        point_name(&w.fx, "refs/heads/feature").as_deref(),
        Some("feature")
    );
    // A suffix means the token no longer names that branch's tip.
    assert_eq!(point_name(&w.fx, "main~2"), None);
    assert_eq!(point_name(&w.fx, "v1"), None, "a tag is not a branch");
    assert_eq!(point_name(&w.fx, "origin/main"), None, "nor is a remote");
    assert_eq!(point(&w.fx, "origin/main"), w.c5, "which still resolves");
    assert_eq!(point_name(&w.fx, &w.c1), None);
    // `trunk` is a revision, and it resolves through fufu's own ladder.
    assert_eq!(point(&w.fx, "trunk"), point(&w.fx, "main"));
    assert_eq!(point_name(&w.fx, "trunk").as_deref(), Some("main"));
}

// --- sets ---

#[test]
fn ancestors_agree_with_rev_list() {
    let w = world();
    assert_eq!(set(&w.fx, "::main"), git_set(&w.fx, &["main"]));
    assert_eq!(set(&w.fx, "::feature"), git_set(&w.fx, &["feature"]));
    assert_eq!(set(&w.fx, "::v1"), git_set(&w.fx, &["v1"]));
    assert_eq!(
        set(&w.fx, "::main | ::feature"),
        git_set(&w.fx, &["main", "feature"]),
        "a union is a merge on commit time, and git orders the same way"
    );
}

#[test]
fn ranges_are_gixs_own_range() {
    let w = world();
    // `a..b` is fufu's set-language range. That it agrees with git's two-dot
    // here is a happy coincidence of spelling, not an inheritance: the set
    // language defines it as `::b` minus `::a`, which is what git computes.
    assert_eq!(
        set(&w.fx, &format!("{}..main", w.c2)),
        git_set(&w.fx, &[&format!("{}..main", w.c2)])
    );
    assert_eq!(
        set(&w.fx, "feature..main"),
        git_set(&w.fx, &["feature..main"])
    );
    // An absent left endpoint excludes nothing.
    assert_eq!(set(&w.fx, "..main"), git_set(&w.fx, &["main"]));
    // An absent right endpoint is every visible head.
    assert_eq!(
        set(&w.fx, &format!("{}..", w.c2)),
        git_set(&w.fx, &["--all", &format!("^{}", w.c2)])
    );
}

#[test]
fn complement_of_an_ancestor_set_is_gits_exclusion() {
    let w = world();
    assert_eq!(
        set(&w.fx, "~(::feature)"),
        git_set(&w.fx, &["--all", "^feature"]),
        "hidden paints a tip and its ancestors unwanted, which is exactly `::x`"
    );
    assert_eq!(
        set(&w.fx, "::main & ~(::feature)"),
        git_set(&w.fx, &["main", "^feature"]),
        "the fold: `b` goes into the walk's own hidden list"
    );
    // Deliberate divergence: `~x` is *exact* complement, not git's exclusion.
    // `~main` drops the one commit `main` names; git has no spelling for it,
    // and `~(::main)` above is the one that drops the ancestry too.
    let all = git_set(&w.fx, &["--all"]);
    let without_tip: Vec<String> = all.iter().filter(|id| **id != w.merge).cloned().collect();
    assert_eq!(set(&w.fx, "~main"), without_tip);
}

#[test]
fn intersection_and_union_are_sets() {
    let w = world();
    assert_eq!(
        set(&w.fx, "::main & ::feature"),
        git_set(&w.fx, &["feature"])
    );
    assert_eq!(set(&w.fx, "main | main"), vec![w.merge.clone()], "deduped");
    assert_eq!(set(&w.fx, "main & feature"), Vec::<String>::new());
}

/// The open-ended forward form. Honestly linear — it builds the child map in
/// one pass over visible history — and git has no single spelling for it, so
/// the comparison composes two git commands instead of one.
#[test]
fn descendants_agree_with_a_composed_git_answer() {
    let w = world();
    let expected: Vec<String> = git_set(&w.fx, &["--all"])
        .into_iter()
        .filter(|id| {
            w.fx.try_git(&["merge-base", "--is-ancestor", &w.c2, id])
                .status
                .success()
        })
        .collect();
    assert_eq!(set(&w.fx, &format!("{}::", w.c2)), expected);

    // Bounded on the right, it stops at the ceiling.
    let bounded: Vec<String> = git_set(&w.fx, &["feature"])
        .into_iter()
        .filter(|id| {
            w.fx.try_git(&["merge-base", "--is-ancestor", &w.c2, id])
                .status
                .success()
        })
        .collect();
    assert_eq!(set(&w.fx, &format!("{}::feature", w.c2)), bounded);
}

#[test]
fn functions_pick_out_the_edges_of_a_set() {
    let w = world();
    assert_eq!(set(&w.fx, "latest(::main)"), vec![w.merge.clone()]);
    assert_eq!(set(&w.fx, "heads(::main)"), vec![w.merge.clone()]);
    assert_eq!(set(&w.fx, "roots(::main)"), vec![w.c1.clone()]);
    assert_eq!(
        set(&w.fx, "heads(::main | ::feature)"),
        vec![w.merge.clone()],
        "the merge swallows the feature tip"
    );
    assert_eq!(
        set(&w.fx, &format!("heads({} | {} | main)", w.c1, w.c3)),
        vec![w.merge.clone()]
    );
    assert_eq!(
        set(&w.fx, &format!("roots({} | {} | main)", w.c1, w.c3)),
        vec![w.c1.clone()]
    );
}

#[test]
fn predicates_agree_with_git_log() {
    let w = world();
    let by_message = git_set(&w.fx, &["--all", "--grep=feature"]);
    assert_eq!(set(&w.fx, "description(substring:feature)"), by_message);
    // A bare argument's implicit kind is the calling function's call.
    assert_eq!(set(&w.fx, "description(feature)"), by_message);
    assert_eq!(
        set(&w.fx, "description(glob:*feature*)"),
        by_message,
        "glob and substring agree when the glob is anchored nowhere"
    );
    assert_eq!(
        set(&w.fx, r#"description(exact:"nothing like this")"#).len(),
        0
    );

    let everything = git_set(&w.fx, &["--all"]);
    assert_eq!(
        set(&w.fx, "author(substring:Fixture)"),
        everything,
        "one author wrote the whole fixture"
    );
    assert_eq!(set(&w.fx, "author(glob:*@fixture.test)"), everything);
}

// --- the open change ---

#[test]
fn the_open_change_is_fufus_and_not_gits() {
    let w = world();
    let repo = w.fx.repo();
    // Deliberate divergence: git's `@` is HEAD's commit; fufu's is the open
    // change, which has no id yet. gix's own `@`-means-HEAD deviation is
    // structurally unreachable because a bare `@` never reaches it.
    let members = set(&w.fx, "@");
    assert_eq!(members, vec!["@".to_string()]);
    assert!(matches!(
        Revset::parse("@")
            .expect("parses")
            .point(&repo)
            .expect("point")
            .rev,
        Rev::Open
    ));

    // A walk rooted at `@` is rooted at HEAD's commit, with the open change
    // riding in front of it — it sorts first because it is the newest thing.
    let mut expected = vec!["@".to_string()];
    expected.extend(git_set(&w.fx, &["HEAD"]));
    assert_eq!(set(&w.fx, "::@"), expected);

    // And the frontier stays a frontier: taking three costs three.
    assert_eq!(set(&w.fx, "::@").into_iter().take(3).count(), 3);
}

// --- refusals ---

#[test]
fn the_open_change_takes_no_suffixes() {
    let w = world();
    for src in ["@^", "@~2", "@@{1}"] {
        assert_eq!(refusal(&w.fx, src), "usage/revset-open-suffix", "{src}");
    }
    // What the message teaches works.
    assert_eq!(point(&w.fx, "HEAD"), git_point(&w.fx, "HEAD"));
}

#[test]
fn rev_list_range_shorthands_are_refused_by_name() {
    let w = world();
    // `x^!` and `x^@` are rev-list ranges wearing a suffix's clothes. git
    // prints several lines for both, which is the tell.
    assert!(w.fx.git(&["rev-parse", "main^!"]).lines().count() > 1);
    assert_eq!(refusal(&w.fx, "main^!"), "usage/revset-range-suffix");
    assert_eq!(refusal(&w.fx, "main^@"), "usage/revset-range-suffix");
}

#[test]
fn a_revset_is_a_set_of_commits() {
    let w = world();
    // git resolves both of these; fufu's revsets are commit space, so a tree
    // and a blob are refused rather than silently peeled to something else.
    assert!(!w.fx.git(&["rev-parse", "main^{tree}"]).trim().is_empty());
    assert_eq!(refusal(&w.fx, "main^{tree}"), "usage/revset-not-a-commit");
    // The canonical-base invariant is what refuses these: only a full ref
    // path, a full sha, or `HEAD` is ever handed to gix.
    assert_eq!(
        refusal(&w.fx, "HEAD:a.txt"),
        "usage/revset-unknown-revision"
    );
    assert_eq!(
        refusal(&w.fx, ":/two"),
        "usage/revset-unknown-revision",
        "the fufu spelling is description(substring:two)"
    );
    assert_eq!(set(&w.fx, "description(substring:two)"), vec![w.c2.clone()]);
}

#[test]
fn ambiguity_is_refused_and_names_both_spellings() {
    let w = world();
    // A branch spelled exactly like an object abbreviation: git picks one
    // and warns; fufu refuses and names the two spellings that decide it.
    let short = w.c1[..8].to_string();
    w.fx.git(&["branch", &short, "main"]);
    let repo = w.fx.repo();
    let err = Revset::parse(&short)
        .expect("parses")
        .point(&repo)
        .expect_err("refused");
    assert_eq!(err.id(), "usage/revset-ambiguous");
    let msg = err.to_string();
    assert!(msg.contains(&format!("refs/heads/{short}")), "{msg}");
    assert!(msg.contains(&w.c1), "must name the full sha: {msg}");
    assert_eq!(err.exits().len(), 2, "the ref path and the sha");
}

#[test]
fn an_operation_in_a_revision_position_is_taught_not_mystified() {
    let fx = Fixture::new();
    fx.write("a.txt", "1\n");
    fx.commit("one");
    // An operation only exists once there is something to capture.
    fx.write("a.txt", "1 and more\n");
    let repo = fx.repo();
    let op = ff_core::ops::capture(
        &repo,
        &ff_core::Provenance::new("manual", None),
        &ff_core::TakeOptions {
            now: Some(1_700_000_000),
            max_file_size: None,
        },
    )
    .expect("capture");
    let id = match op {
        ff_core::ops::CaptureOutcome::Created { id, .. } => id,
        other => panic!("expected a capture, got {other:?}"),
    };

    // The letters spelling, in a revision position.
    let err = Revset::parse(&id.to_string())
        .expect("parses")
        .point(&repo)
        .expect_err("refused");
    assert_eq!(err.id(), "usage/op-in-rev-position");
    assert!(err.to_string().contains("--at-op"), "{err}");

    // And the raw sha, which reaches the same commit the long way round.
    let err = Revset::parse(&id.hex())
        .expect("parses")
        .point(&repo)
        .expect_err("refused");
    assert_eq!(err.id(), "usage/op-in-rev-position");
}

#[test]
fn functions_are_refused_by_signature() {
    let w = world();
    assert_eq!(
        refusal(&w.fx, "nosuch(main)"),
        "usage/revset-unknown-function"
    );
    assert_eq!(refusal(&w.fx, "latest()"), "usage/revset-arity");
    assert_eq!(refusal(&w.fx, "latest(main, main)"), "usage/revset-arity");
    assert_eq!(refusal(&w.fx, "latest(glob:x)"), "usage/revset-arity");
    assert_eq!(
        refusal(&w.fx, "description(main | feature)"),
        "usage/revset-arity"
    );
    // Recognized and refused for a reason of its own: the depth-bounded
    // forward form needs an index git has no child edges to build.
    assert_eq!(
        refusal(&w.fx, "descendants(main, 2)"),
        "revset/deferred-descendants"
    );
    // Recognized and refused: the name belongs to the other address space.
    for src in ["base(main)", "on_branch(main)", "session(x)", "kind(op)"] {
        assert_eq!(refusal(&w.fx, src), "usage/revset-wrong-space", "{src}");
    }
    assert_eq!(
        refusal(&w.fx, r#"description(regex:"^fix")"#),
        "revset/regex-unavailable"
    );
}

#[test]
fn the_refused_spellings_from_the_front_end_still_reach_a_reader() {
    let w = world();
    assert_eq!(refusal(&w.fx, "main+"), "revset/deferred-descendants");
    assert_eq!(refusal(&w.fx, "main-"), "usage/revset-parent-shorthand");
    assert_eq!(
        refusal(&w.fx, "main...feature"),
        "usage/revset-no-symmetric-difference",
        "a set language already says it as `(a..b) | (b..a)`"
    );
    // And the spelling the message names does work: the symmetric difference
    // is exactly the two one-sided ranges unioned.
    let mut expected = git_set(&w.fx, &["feature..main"]);
    expected.extend(git_set(&w.fx, &["main..feature"]));
    expected.sort();
    let mut got = set(&w.fx, "(feature..main) | (main..feature)");
    got.sort();
    assert_eq!(got, expected);
}

#[test]
fn a_point_is_exactly_one_or_an_error() {
    let w = world();
    let repo = w.fx.repo();
    let err = Revset::parse("::main")
        .expect("parses")
        .point(&repo)
        .expect_err("many");
    assert_eq!(err.id(), "usage/revset-not-a-point");
    assert!(err.to_string().contains("more than one"), "{err}");
    assert!(
        err.exits().iter().any(|e| e.contains("latest(")),
        "must name the spelling that narrows it: {:?}",
        err.exits()
    );
    assert!(err.exits().iter().any(|e| e.contains("heads(")));

    let err = Revset::parse("main & feature")
        .expect("parses")
        .point(&repo)
        .expect_err("none");
    assert_eq!(err.id(), "usage/revset-empty-set");
}

#[test]
fn an_unborn_repository_answers_rather_than_failing() {
    let fx = Fixture::new();
    let repo = fx.repo();
    // Nothing is committed, so `@` is still the open change and every set
    // rooted in history is empty. That is a fact, not an error.
    assert!(matches!(
        Revset::parse("@")
            .expect("parses")
            .point(&repo)
            .expect("point")
            .rev,
        Rev::Open
    ));
    assert_eq!(set(&fx, "::@"), vec!["@".to_string()]);
    assert_eq!(refusal(&fx, "main"), "usage/revset-unknown-revision");
}
