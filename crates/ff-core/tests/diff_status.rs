//! The status fixture matrix, table-driven: every scenario is built with real
//! git, then ff-core's status is compared against `git status --porcelain=v2
//! --branch` — including the `.git/index` byte-identity tripwire.

use ff_testsupport::Fixture;
use ff_testsupport::porcelain::assert_status_matches;
use ff_testsupport::scenarios;

#[test]
fn status_matrix() {
    for (name, build) in scenarios() {
        println!("scenario: {name}");
        let fx = Fixture::new();
        build(&fx);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            assert_status_matches(&fx);
        }));
        if let Err(err) = result {
            let msg = err
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "non-string panic".into());
            panic!("scenario '{name}' failed: {msg}");
        }
    }
}

/// Repeated status calls stay byte-identical — nothing accumulates.
#[test]
fn status_is_idempotent() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "changed\n");
    fx.write("new.txt", "untracked\n");
    let before = fx.index_bytes();
    for _ in 0..3 {
        assert_status_matches(&fx);
    }
    assert_eq!(before, fx.index_bytes());
}
