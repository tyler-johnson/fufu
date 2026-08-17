//! The map — a skeleton of the local branches that answers "where did I
//! leave that idea?": the tips, the merges, the forks, and the roots, with
//! the linear runs between them contracted into elision nodes. The walk
//! stops where the frontier converges on one commit or `fufu.mapDepth` runs
//! out; it is a pure read and knows nothing about rendering.

use std::collections::{BinaryHeap, HashMap, HashSet};

use gix::prelude::ObjectIdExt;
use serde::Serialize;

use crate::branch::is_anonymous;
use crate::error::{Error, Result};

/// Which branches the map ranks in, before the forced ones are appended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct MapOptions {
    /// How many local branches to rank in; `None` is every one.
    pub branches: Option<usize>,
}

/// The skeleton itself, newest to oldest, with the open change on top.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Map {
    pub rows: Vec<MapRow>,
    /// The walk stopped on `fufu.mapDepth` rather than converging.
    pub truncated: bool,
}

/// One row of the skeleton.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MapRow {
    pub node: MapNode,
    /// Row indices of this node's parents. Always greater than the row's own
    /// index, so the rows read top-down as newest-to-oldest.
    pub parents: Vec<usize>,
}

/// The three shapes a skeleton row can take.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum MapNode {
    Open {
        branch: String,
        id: Option<String>,
        subject: Option<String>,
        pending: Option<String>,
        time: Option<i64>,
        clean: bool,
        born: bool,
    },
    Commit {
        id: String,
        short_id: String,
        subject: String,
        time: i64,
        refs: Vec<MapRef>,
    },
    Elided {
        /// Commits contracted away; `None` marks a frontier — parents this
        /// walk did not follow.
        count: Option<usize>,
    },
}

/// A branch's claim on a commit node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MapRef {
    pub name: String,
    pub current: bool,
    pub anonymous: bool,
    /// Files in the parked change waiting on this branch, when one is.
    pub parked: Option<usize>,
    pub pending_description: Option<String>,
}

/// One commit the walk read: what ranking, visibility, and contraction need
/// of it. `children` counts walked children, because a fork point is a
/// commit that two walked children descend through.
struct Visit {
    time: i64,
    parents: Vec<gix::ObjectId>,
    children: usize,
}

/// A node of the contracted graph, before its parent list becomes row
/// indices.
struct Built {
    node: MapNode,
    parents: Vec<usize>,
    /// The newest commit this node stands for; the ordering step sorts on it.
    sort_time: i64,
}

/// The committer time of a commit, seconds since the epoch. The map ranks and
/// sorts on the committer clock, not the author clock `log` displays.
fn committer_time(repo: &gix::Repository, id: gix::ObjectId) -> Result<i64> {
    Ok(repo
        .find_commit(id)
        .map_err(Error::repo)?
        .committer()
        .map_err(Error::repo)?
        .time()
        .map_err(Error::repo)?
        .seconds)
}

/// Read one commit and record it: time, and parents in recorded order.
fn read_visit(
    repo: &gix::Repository,
    id: gix::ObjectId,
    order: &mut Vec<gix::ObjectId>,
    visited: &mut HashMap<gix::ObjectId, Visit>,
) -> Result<()> {
    let commit = repo.find_commit(id).map_err(Error::repo)?;
    let time = commit
        .committer()
        .map_err(Error::repo)?
        .time()
        .map_err(Error::repo)?
        .seconds;
    let parents: Vec<gix::ObjectId> = commit.parent_ids().map(|id| id.detach()).collect();
    order.push(id);
    visited.insert(
        id,
        Visit {
            time,
            parents,
            children: 0,
        },
    );
    Ok(())
}

/// The skeleton of the repository's local branches, newest to oldest, with
/// the open change as row 0. A pure read: it raises only repository errors.
pub fn map(repo: &gix::Repository, opts: &MapOptions) -> Result<Map> {
    // --- 1. Tips: who is in the map, and in what order.

    let head = crate::head::head_state(repo)?;
    let current = crate::snapshot::chain::chain_name(&head);

    let mut tips: Vec<(String, gix::ObjectId, i64)> = Vec::new();
    for name in crate::switch::branch_names(repo)? {
        let Some(id) = crate::refs::ref_target(repo, &format!("refs/heads/{name}"))? else {
            continue;
        };
        tips.push((name, id, committer_time(repo, id)?));
    }
    // Newest tip first; equal times are a clock accident the name breaks.
    tips.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));

    // Forced names are present at whatever rank: the branch we stand on, and
    // a local trunk. A trunk with no local branch has no ref to walk, and a
    // trunk we cannot resolve is no reason to fail a read.
    let mut forced: Vec<String> = Vec::new();
    if tips.iter().any(|(n, _, _)| n == &current) {
        forced.push(current.clone());
    }
    if let Ok(t) = crate::trunk::trunk(repo)
        && matches!(t.kind, crate::trunk::TrunkKind::Local)
        && tips.iter().any(|(n, _, _)| n == &t.name)
        && !forced.contains(&t.name)
    {
        forced.push(t.name);
    }

    let mut picked: Vec<String> = tips
        .iter()
        .take(opts.branches.unwrap_or(usize::MAX))
        .map(|(n, _, _)| n.clone())
        .collect();
    for name in &forced {
        if !picked.iter().any(|n| n == name) {
            picked.push(name.clone());
        }
    }

    // The reachability mask below is 64 bits: bit i belongs to source i, and
    // a detached HEAD holds one. Cap the selection so the mask never
    // overflows; the forced names always fit.
    let is_detached = matches!(head, crate::model::HeadState::Detached { .. });
    let budget = if is_detached { 63 } else { 64 };
    if picked.len() > budget {
        let kept_forced: Vec<String> = forced
            .iter()
            .filter(|f| picked.iter().any(|n| n == *f))
            .cloned()
            .collect();
        let mut kept: Vec<String> = Vec::new();
        for name in &picked {
            if kept_forced.iter().any(|f| f == name) {
                continue;
            }
            if kept.len() < budget.saturating_sub(kept_forced.len()) {
                kept.push(name.clone());
            }
        }
        kept.extend(kept_forced);
        picked = kept;
    }

    // Sources are the distinct tips of the selection, in first-appearance
    // order; two branches may share one tip, but a tip is one bit.
    let mut sources: Vec<gix::ObjectId> = Vec::new();
    for name in &picked {
        if let Some((_, id, _)) = tips.iter().find(|(n, _, _)| n == name)
            && !sources.contains(id)
        {
            sources.push(*id);
        }
    }
    // A detached HEAD has no branch name to walk under; force its commit in
    // so the @ row has a parent node to sit on.
    if let crate::model::HeadState::Detached { commit } = &head {
        let id = gix::ObjectId::from_hex(commit.as_bytes()).map_err(Error::repo)?;
        if !sources.contains(&id) {
            sources.push(id);
        }
    }

    // --- 2. The walk: newest first, following parents to convergence.

    let depth_cap = repo
        .config_snapshot()
        .integer("fufu.mapDepth")
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(1000);
    let full_mask = if sources.len() == 64 {
        u64::MAX
    } else {
        (1u64 << sources.len()) - 1
    };

    let mut heap: BinaryHeap<(i64, String)> = BinaryHeap::new();
    let mut queued: HashSet<gix::ObjectId> = HashSet::new();
    let mut reach: HashMap<gix::ObjectId, u64> = HashMap::new();
    for (i, id) in sources.iter().enumerate() {
        heap.push((committer_time(repo, *id)?, id.to_string()));
        queued.insert(*id);
        reach.insert(*id, 1u64 << i);
    }

    let mut truncated = false;
    let mut order: Vec<gix::ObjectId> = Vec::new();
    let mut visited: HashMap<gix::ObjectId, Visit> = HashMap::new();
    while let Some((_, id_hex)) = heap.peek() {
        if visited.len() >= depth_cap {
            truncated = true;
            break;
        }
        let id = gix::ObjectId::from_hex(id_hex.as_bytes()).map_err(Error::repo)?;
        // Convergence: one commit left in the frontier, known to every
        // source. It is the fork the whole map hangs from; put it on the
        // table and stop before its parents.
        if heap.len() == 1 && reach.get(&id).is_some_and(|&mask| mask == full_mask) {
            read_visit(repo, id, &mut order, &mut visited)?;
            break;
        }
        heap.pop();
        read_visit(repo, id, &mut order, &mut visited)?;
        let mask = reach[&id];
        let parents = visited[&id].parents.clone();
        for p in parents {
            *reach.entry(p).or_insert(0) |= mask;
            if !visited.contains_key(&p) && !queued.contains(&p) {
                heap.push((committer_time(repo, p)?, p.to_string()));
                queued.insert(p);
            }
        }
    }

    // --- 3. Visibility: what is structure in the walked set.

    {
        let mut children: HashMap<gix::ObjectId, usize> = HashMap::new();
        for visit in visited.values() {
            for p in &visit.parents {
                if visited.contains_key(p) {
                    *children.entry(*p).or_insert(0) += 1;
                }
            }
        }
        for (id, count) in children {
            if let Some(visit) = visited.get_mut(&id) {
                visit.children = count;
            }
        }
    }

    let source_set: HashSet<gix::ObjectId> = sources.iter().copied().collect();
    let mut visible: HashSet<gix::ObjectId> = visited
        .iter()
        .filter(|(id, v)| {
            source_set.contains(*id)
                || v.parents.len() >= 2
                || v.children >= 2
                || v.parents.is_empty()
        })
        .map(|(id, _)| *id)
        .collect();

    // --- 4a. Runs of exactly one commit are shown, not elided: an elision
    // that saves no line and tells less is a bad trade. One sweep, because a
    // promoted commit belongs to exactly one run and cannot spawn another.
    for (id, v) in &visited {
        if !visible.contains(id) {
            continue;
        }
        for p in &v.parents {
            let mut run = Vec::new();
            let mut cur = Some(*p);
            while let Some(c) = cur {
                match visited.get(&c) {
                    Some(cv) if !visible.contains(&c) => {
                        run.push(c);
                        cur = cv.parents.first().copied();
                    }
                    _ => break,
                }
            }
            if run.len() == 1 {
                visible.insert(run[0]);
            }
        }
    }

    // --- 7. Decorations, attached while the commit nodes are built.

    // One MapRef per selected branch, on the node its tip resolves to. The
    // current branch's name is omitted: it lives on the @ row, and the tip
    // below must not repeat it.
    let mut refs_by_tip: HashMap<gix::ObjectId, Vec<MapRef>> = HashMap::new();
    for name in &picked {
        if name == &current {
            continue;
        }
        let Some((_, id, _)) = tips.iter().find(|(n, _, _)| n == name) else {
            continue;
        };
        // A parked change is a decoration, not structure: one branch's
        // unreadable stash must never fail the whole map.
        let parked = crate::stash::parked_entry(repo, name)
            .ok()
            .flatten()
            .and_then(|stash_id| {
                crate::stash::read_stash_commit(repo, stash_id)
                    .and_then(|stash| {
                        crate::changestat::tree_diff_stat(repo, stash.base_tree, stash.wip_tree)
                    })
                    .map(|stat| stat.files.len())
                    .ok()
            });
        let pending_description = crate::branchmeta::read(repo, name)?.pending_description;
        refs_by_tip.entry(*id).or_default().push(MapRef {
            name: name.clone(),
            // The current branch was skipped above, so no ref here is.
            current: false,
            anonymous: is_anonymous(name),
            parked,
            pending_description,
        });
    }

    // --- 4b. Contract the runs; nodes are built in walk order, parents in
    // recorded order, so construction index is deterministic.

    let mut built: Vec<Built> = Vec::new();
    let mut commit_node: HashMap<gix::ObjectId, usize> = HashMap::new();
    for &id in &order {
        if !visible.contains(&id) {
            continue;
        }
        let commit = repo.find_commit(id).map_err(Error::repo)?;
        let short_id = id.attach(repo).shorten().map_err(Error::repo)?.to_string();
        let subject = commit.message().map_err(Error::repo)?.summary().to_string();
        let time = visited[&id].time;
        let mut refs = refs_by_tip.get(&id).cloned().unwrap_or_default();
        refs.sort_by(|a, b| a.name.cmp(&b.name));
        commit_node.insert(id, built.len());
        built.push(Built {
            node: MapNode::Commit {
                id: id.to_string(),
                short_id,
                subject,
                time,
                refs,
            },
            parents: Vec::new(),
            sort_time: time,
        });
    }

    for &id in &order {
        if !visible.contains(&id) {
            continue;
        }
        let vidx = commit_node[&id];
        // A visible node gets at most one frontier marker: it means "history
        // continues past here", not "this particular parent".
        let mut frontier_marked = false;
        for p in &visited[&id].parents {
            let mut run = Vec::new();
            let mut cur = Some(*p);
            while let Some(c) = cur {
                match visited.get(&c) {
                    Some(cv) if !visible.contains(&c) => {
                        run.push(c);
                        cur = cv.parents.first().copied();
                    }
                    _ => break,
                }
            }
            if let Some(end) = cur {
                if visible.contains(&end) {
                    let end_idx = match commit_node.get(&end) {
                        // A visible commit always got a node above.
                        Some(&i) => i,
                        None => continue,
                    };
                    if run.is_empty() {
                        built[vidx].parents.push(end_idx);
                    } else {
                        let e = built.len();
                        built.push(Built {
                            node: MapNode::Elided {
                                count: Some(run.len()),
                            },
                            parents: vec![end_idx],
                            // The run's newest commit is the one nearest the child.
                            sort_time: visited[&run[0]].time,
                        });
                        built[vidx].parents.push(e);
                    }
                } else {
                    if frontier_marked {
                        continue;
                    }
                    frontier_marked = true;
                    if run.is_empty() {
                        let f = built.len();
                        built.push(Built {
                            node: MapNode::Elided { count: None },
                            parents: Vec::new(),
                            sort_time: visited[&id].time - 1,
                        });
                        built[vidx].parents.push(f);
                    } else {
                        let f = built.len();
                        built.push(Built {
                            node: MapNode::Elided { count: None },
                            parents: Vec::new(),
                            sort_time: visited[&id].time - 1,
                        });
                        let e = built.len();
                        built.push(Built {
                            node: MapNode::Elided {
                                count: Some(run.len()),
                            },
                            parents: vec![f],
                            sort_time: visited[&run[0]].time,
                        });
                        built[vidx].parents.push(e);
                    }
                }
            } else if !frontier_marked {
                // No parent at all is a root, and roots are visible, so this
                // arm is unreachable; the marker rule still owns it.
                frontier_marked = true;
            }
        }
    }

    // --- 5. Ordering: topological, newest sort_time first, construction
    // order breaking ties.

    let n = built.len();
    let mut pending = vec![0usize; n];
    for b in &built {
        for &p in &b.parents {
            pending[p] += 1;
        }
    }
    let mut ready: BinaryHeap<(i64, std::cmp::Reverse<usize>, usize)> = BinaryHeap::new();
    for i in 0..n {
        if pending[i] == 0 {
            ready.push((built[i].sort_time, std::cmp::Reverse(i), i));
        }
    }
    let mut emitted: Vec<usize> = Vec::with_capacity(n);
    while let Some((_, _, i)) = ready.pop() {
        emitted.push(i);
        for &p in &built[i].parents {
            pending[p] -= 1;
            if pending[p] == 0 {
                ready.push((built[p].sort_time, std::cmp::Reverse(p), p));
            }
        }
    }
    // The contracted graph is a DAG, so the heap reaches every node; append
    // any it cannot rather than drop a row.
    if emitted.len() < n {
        for i in 0..n {
            if !emitted.contains(&i) {
                emitted.push(i);
            }
        }
    }
    // Rows are emitted starting at index 1: the @ row takes row 0.
    let mut pos: HashMap<usize, usize> = HashMap::new();
    for (row, node) in emitted.iter().enumerate() {
        pos.insert(*node, row + 1);
    }

    // --- 6. The @ row, and the tip commit it stands on.

    let open = crate::evolog::open_change(repo)?;
    let head_tip = match &head {
        crate::model::HeadState::Unborn { .. } => None,
        crate::model::HeadState::Branch { .. } => {
            crate::refs::ref_target(repo, &format!("refs/heads/{current}"))?
        }
        crate::model::HeadState::Detached { commit } => {
            Some(gix::ObjectId::from_hex(commit.as_bytes()).map_err(Error::repo)?)
        }
    };
    let mut open_parent: Option<usize> = None;
    if let Some(id) = head_tip
        && let Some(&node) = commit_node.get(&id)
        && let Some(&row) = pos.get(&node)
    {
        open_parent = Some(row);
    }

    let mut rows = Vec::with_capacity(n + 1);
    rows.push(MapRow {
        node: MapNode::Open {
            branch: open.branch,
            id: open.id,
            subject: open.subject,
            pending: open.pending,
            time: open.time,
            clean: open.clean,
            born: open.base.is_some(),
        },
        parents: open_parent.map(|r| vec![r]).unwrap_or_default(),
    });
    for node in &emitted {
        rows.push(MapRow {
            node: built[*node].node.clone(),
            parents: built[*node].parents.iter().map(|p| pos[p]).collect(),
        });
    }

    Ok(Map { rows, truncated })
}
