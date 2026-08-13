//! Scratch profiler for `ff status` phases. Run in a repo:
//! `cargo run --release -p ff-core --example profile_status`

use std::time::Instant;

fn main() {
    let t = Instant::now();
    let repo = ff_core::discover(".").unwrap();
    println!("discover:        {:?}", t.elapsed());

    let t = Instant::now();
    let index = repo.index_or_empty().unwrap();
    println!(
        "open index:      {:?} ({} entries)",
        t.elapsed(),
        index.entries().len()
    );

    let t = Instant::now();
    let head = ff_core::head_state(&repo).unwrap();
    println!("head:            {:?} ({head:?})", t.elapsed());

    let t = Instant::now();
    let upstream = ff_core::upstream(&repo).unwrap();
    println!("upstream:        {:?} ({upstream:?})", t.elapsed());

    let t = Instant::now();
    let head_tree = repo.head_tree_id_or_empty().unwrap();
    let _index_from_tree = repo.index_from_tree(&head_tree).unwrap();
    println!("index_from_tree: {:?}", t.elapsed());

    for round in 0..2 {
        let t = Instant::now();
        let iter = repo
            .status(gix::progress::Discard)
            .unwrap()
            .index_worktree_rewrites(None)
            .into_iter(None::<gix::bstr::BString>)
            .unwrap();
        let n = iter.count();
        println!("status iter #{round}:  {:?} ({n} items)", t.elapsed());
    }

    let t = Instant::now();
    let iter = repo
        .status(gix::progress::Discard)
        .unwrap()
        .index_worktree_rewrites(None)
        .into_index_worktree_iter(Vec::new())
        .unwrap();
    let n = iter.count();
    println!("iw only:         {:?} ({n} items)", t.elapsed());

    let t = Instant::now();
    let iter = repo
        .status(gix::progress::Discard)
        .unwrap()
        .index_worktree_rewrites(None)
        .untracked_files(gix::status::UntrackedFiles::None)
        .into_index_worktree_iter(Vec::new())
        .unwrap();
    let n = iter.count();
    println!("iw no-untracked: {:?} ({n} items)", t.elapsed());

    let t = Instant::now();
    let mut n = 0usize;
    repo.tree_index_status(
        &head_tree,
        &index,
        None,
        gix::status::tree_index::TrackRenames::default(),
        |_change, _, _| {
            n += 1;
            Ok::<_, std::convert::Infallible>(gix::diff::index::Action::Continue)
        },
    )
    .unwrap();
    println!("tree-index only: {:?} ({n} changes)", t.elapsed());

    for limit in [None, Some(1), Some(2), Some(3), Some(4)] {
        let t = Instant::now();
        let iter = repo
            .status(gix::progress::Discard)
            .unwrap()
            .index_worktree_rewrites(None)
            .index_worktree_options_mut(|opts| opts.thread_limit = limit)
            .into_index_worktree_iter(Vec::new())
            .unwrap();
        let n = iter.count();
        println!("iw threads {limit:?}:  {:?} ({n} items)", t.elapsed());
    }

    let t = Instant::now();
    let status = ff_core::status(&repo).unwrap();
    println!(
        "full status:     {:?} ({} unstaged, {} untracked)",
        t.elapsed(),
        status.unstaged.len(),
        status.untracked.len()
    );

    // With index.skipHash=true override: how much of the open cost is SHA verify?
    let opts = gix::open::Options::default().config_overrides(["index.skipHash=true"]);
    let repo2 = gix::ThreadSafeRepository::discover_opts(
        ".",
        Default::default(),
        gix::sec::trust::Mapping {
            full: opts.clone(),
            reduced: opts,
        },
    )
    .unwrap()
    .to_thread_local();
    let t = Instant::now();
    let index = repo2.open_index().unwrap();
    println!(
        "open skipHash:   {:?} ({} entries)",
        t.elapsed(),
        index.entries().len()
    );
    drop(index);
    let t = Instant::now();
    let status = ff_core::status(&repo2).unwrap();
    println!(
        "full skipHash:   {:?} ({} unstaged)",
        t.elapsed(),
        status.unstaged.len()
    );
}
