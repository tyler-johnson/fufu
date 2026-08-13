//! The shared fixture matrix: every working-tree shape the differential
//! contract must hold for. Used by both the status matrix (`diff_status.rs`)
//! and the snapshot matrix (`diff_snapshot.rs`).

use crate::fixtures::Fixture;

pub type Scenario = (&'static str, fn(&Fixture));

pub fn scenarios() -> Vec<Scenario> {
    vec![
        ("unborn", |_fx| {}),
        ("unborn_dirty", |fx| {
            fx.write("staged.txt", "staged\n");
            fx.git(&["add", "staged.txt"]);
            fx.write("loose.txt", "untracked\n");
        }),
        ("clean_with_history", |fx| {
            fx.write("a.txt", "a\n");
            fx.write("dir/b.txt", "b\n");
            fx.commit("init");
        }),
        ("detached_dirty", |fx| {
            fx.write("a.txt", "a\n");
            let first = fx.commit("one");
            fx.write("a.txt", "aa\n");
            fx.commit("two");
            fx.git(&["checkout", "-q", &first]);
            fx.write("a.txt", "dirty\n");
        }),
        ("staged_only", |fx| {
            fx.write("a.txt", "a\n");
            fx.commit("init");
            fx.write("new.txt", "new\n");
            fx.write("a.txt", "changed\n");
            fx.git(&["add", "-A"]);
        }),
        ("staged_delete", |fx| {
            fx.write("a.txt", "a\n");
            fx.commit("init");
            fx.git(&["rm", "-q", "a.txt"]);
        }),
        ("unstaged_only", |fx| {
            fx.write("a.txt", "a\n");
            fx.write("b.txt", "b\n");
            fx.commit("init");
            fx.write("a.txt", "changed\n");
            fx.remove("b.txt");
        }),
        ("untracked_nested_collapses", |fx| {
            fx.write("tracked.txt", "t\n");
            fx.commit("init");
            fx.write("a/b/c.txt", "c\n");
            fx.write("a/d.txt", "d\n");
            fx.write("top.txt", "top\n");
        }),
        ("ignored_appears_nowhere", |fx| {
            fx.write(".gitignore", "ignored.txt\nignored-dir/\n");
            fx.commit("init");
            fx.write("ignored.txt", "x\n");
            fx.write("ignored-dir/deep.txt", "x\n");
        }),
        ("mixed", |fx| {
            fx.write("a.txt", "a\n");
            fx.write("b.txt", "b\n");
            fx.commit("init");
            fx.write("a.txt", "staged\n");
            fx.git(&["add", "a.txt"]);
            fx.write("a.txt", "staged then modified again\n");
            fx.write("b.txt", "unstaged\n");
            fx.write("new/nested.txt", "untracked\n");
        }),
        ("slash_branch", |fx| {
            fx.write("a.txt", "a\n");
            fx.commit("init");
            fx.git(&["checkout", "-q", "-b", "feat/x/y"]);
            fx.write("a.txt", "changed\n");
            fx.git(&["add", "a.txt"]);
        }),
        ("packed_refs", |fx| {
            fx.write("a.txt", "a\n");
            fx.commit("init");
            fx.git(&["pack-refs", "--all"]);
            fx.write("a.txt", "changed\n");
        }),
        ("staged_rename", |fx| {
            fx.write("old-name.txt", "same content, long enough to match\n");
            fx.write("other.txt", "other\n");
            fx.commit("init");
            fx.git(&["mv", "old-name.txt", "new-name.txt"]);
        }),
        ("staged_rename_with_edit", |fx| {
            fx.write(
                "old.txt",
                "line one\nline two\nline three\nline four\nline five\n",
            );
            fx.commit("init");
            fx.git(&["mv", "old.txt", "new.txt"]);
            fx.write(
                "new.txt",
                "line one\nline two\nline three\nline four\nCHANGED\n",
            );
            fx.git(&["add", "new.txt"]);
        }),
        ("intent_to_add", |fx| {
            fx.write("a.txt", "a\n");
            fx.commit("init");
            fx.write("later.txt", "content\n");
            fx.git(&["add", "-N", "later.txt"]);
        }),
        // unix-only: creating symlinks on Windows needs Developer Mode or
        // admin privilege, so the typechange shapes can't be built there.
        #[cfg(unix)]
        ("typechange_unstaged", |fx| {
            fx.write("target.txt", "target\n");
            fx.write("link.txt", "was a file\n");
            fx.commit("init");
            fx.remove("link.txt");
            std::os::unix::fs::symlink("target.txt", fx.path().join("link.txt")).unwrap();
        }),
        #[cfg(unix)]
        ("typechange_staged", |fx| {
            fx.write("target.txt", "target\n");
            fx.write("link.txt", "was a file\n");
            fx.commit("init");
            fx.remove("link.txt");
            std::os::unix::fs::symlink("target.txt", fx.path().join("link.txt")).unwrap();
            fx.git(&["add", "-A"]);
        }),
        // unix-only: a staged exec bit is unix semantics, and under Windows'
        // core.filemode=false the throwaway-index reference recipe cannot
        // preserve staged modes, so the differential comparison is ill-posed.
        #[cfg(unix)]
        ("exec_bit_is_modified_not_typechange", |fx| {
            fx.write("script.sh", "#!/bin/sh\n");
            fx.commit("init");
            fx.git(&["update-index", "--chmod=+x", "script.sh"]);
        }),
        ("conflicted_merge", |fx| {
            fx.write("conflict.txt", "base\n");
            fx.write("peaceful.txt", "fine\n");
            fx.commit("base");
            fx.git(&["checkout", "-q", "-b", "other"]);
            fx.write("conflict.txt", "theirs\n");
            fx.commit("theirs");
            fx.git(&["checkout", "-q", "main"]);
            fx.write("conflict.txt", "ours\n");
            fx.commit("ours");
            let out = fx.try_git(&["merge", "other"]);
            assert!(!out.status.success(), "merge should conflict");
        }),
        ("conflicted_merge_with_other_changes", |fx| {
            fx.write("conflict.txt", "base\n");
            fx.write("peaceful.txt", "fine\n");
            fx.commit("base");
            fx.git(&["checkout", "-q", "-b", "other"]);
            fx.write("conflict.txt", "theirs\n");
            fx.commit("theirs");
            fx.git(&["checkout", "-q", "main"]);
            fx.write("conflict.txt", "ours\n");
            fx.commit("ours");
            let out = fx.try_git(&["merge", "other"]);
            assert!(!out.status.success(), "merge should conflict");
            fx.write("peaceful.txt", "edited during conflict\n");
            fx.write("untracked.txt", "new\n");
        }),
        ("backdated_mass_tree", |fx| {
            for d in 0..5 {
                for f in 0..20 {
                    fx.write(&format!("d{d}/f{f}.txt"), &format!("{d}/{f}\n"));
                }
            }
            fx.commit("many files");
            fx.backdate();
            fx.write("d0/f0.txt", "modified\n");
            fx.write("straggler.txt", "untracked\n");
        }),
    ]
}
