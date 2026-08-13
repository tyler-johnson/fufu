//! Log differential tests against `git log`.

use ff_testsupport::Fixture;
use ff_testsupport::porcelain::assert_log_matches;

#[test]
fn linear_history() {
    let fx = Fixture::new();
    for i in 0..5 {
        fx.write("f.txt", &format!("{i}\n"));
        fx.commit(&format!("commit {i}"));
    }
    assert_log_matches(&fx, None);
}

#[test]
fn limit_applies() {
    let fx = Fixture::new();
    for i in 0..6 {
        fx.write("f.txt", &format!("{i}\n"));
        fx.commit(&format!("commit {i}"));
    }
    assert_log_matches(&fx, Some(3));
    assert_log_matches(&fx, Some(25)); // limit larger than history
}

#[test]
fn merge_history() {
    let fx = Fixture::new();
    fx.write("base.txt", "base\n");
    fx.commit("base");
    fx.git(&["checkout", "-q", "-b", "side"]);
    fx.write("side.txt", "side\n");
    fx.commit("side work");
    fx.git(&["checkout", "-q", "main"]);
    fx.write("main.txt", "main\n");
    fx.commit("main work");
    fx.git(&["merge", "-q", "--no-edit", "side"]);
    assert_log_matches(&fx, None);
    assert_log_matches(&fx, Some(2));
}

#[test]
fn multiline_message_subject() {
    let fx = Fixture::new();
    fx.write("a.txt", "a");
    fx.git(&["add", "-A"]);
    fx.git(&[
        "commit",
        "-q",
        "-m",
        "subject line here",
        "-m",
        "body paragraph\nwith more lines",
    ]);
    assert_log_matches(&fx, None);
}

#[test]
fn unborn_yields_empty_not_error() {
    let fx = Fixture::new();
    let mut repo = fx.repo();
    let entries: Vec<_> = ff_core::log(&mut repo, &ff_core::LogOptions::default())
        .expect("unborn log is not an error")
        .collect();
    assert!(entries.is_empty());
    // Contrast: git itself exits non-zero here.
    let out = fx.try_git(&["log"]);
    assert!(!out.status.success());
}

#[test]
fn detached_head_log() {
    let fx = Fixture::new();
    fx.write("a.txt", "a");
    let first = fx.commit("one");
    fx.write("b.txt", "b");
    fx.commit("two");
    fx.git(&["checkout", "-q", &first]);
    assert_log_matches(&fx, None);
}

#[test]
fn bare_repository_log_works() {
    let fx = Fixture::new();
    fx.write("a.txt", "a");
    fx.commit("one");
    fx.write("b.txt", "b");
    fx.commit("two");
    // Make a bare clone next to it, hermetically.
    let bare = fx.root().join("bare.git");
    fx.git(&[
        "clone",
        "-q",
        "--bare",
        fx.path().to_str().unwrap(),
        bare.to_str().unwrap(),
    ]);
    let mut repo = ff_core::discover_isolated(&bare).unwrap();
    let entries: Vec<_> = ff_core::log(&mut repo, &ff_core::LogOptions::default())
        .unwrap()
        .collect::<ff_core::Result<_>>()
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].subject, "two");
}
