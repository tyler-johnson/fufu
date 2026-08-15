//! Binding and evaluation — the two halves the error budget is split across.
//!
//! Binding resolves every revision leaf and raises every refusal the language
//! has. It is O(leaves) and constructs no walk, which is the whole point: a
//! misspelled branch on a repository with a million commits fails in
//! microseconds, because nothing about being wrong was ever priced by
//! history. Evaluation is lazy, so `-r '::@' -n 25` costs twenty-five pops of
//! a frontier and not one commit more, at any depth.
//!
//! Between them sits a plan, and the plan exists so set algebra can be folded
//! into the walk rather than materialized around it. `x..y` *is* gix's range
//! — tips `y`, hidden `x` — and `a & ~(::b)` folds `b` into the same hidden
//! list, allocating nothing. What cannot fold materializes one side and
//! filters the other, and every node hands back the same
//! `Iterator<Item = Result<Member>>` so no caller learns which it got.
//!
//! The forms that scale with history say so here rather than hiding it.
//! `x::` is the one that has no cheap shape available: git stores parent
//! edges and no child edges, so the only honest implementation is a backward
//! walk bounded by the visible heads, with the child map built in one pass
//! and reachability propagated forward from `x`. `description()` and
//! `author()` are linear for the same reason — a predicate over commit
//! content has no index behind it. DESIGN permits a verb that scales to
//! declare it rather than escape the gate, and this is that declaration.
//!
//! `x..` is the sibling that got off lightly: in a set language it is
//! everything visible minus the ancestors of `x`, which is one lazy walk with
//! every ref as a tip and `x` hidden. No child map, no materialization.

use std::collections::{HashMap, HashSet};

use gix::revision::walk::Sorting;
use gix::traverse::commit::simple::CommitTimeOrder;

use crate::error::{Error, Result};
use crate::ops::CommitId;

use super::Rev;
use super::func::{self, Field};
use super::parse::Expr;
use super::pattern::Pattern;
use super::resolve;

/// One member, carrying the key the whole language orders by. Commit time
/// travels with the member because a walk already knows it — recomputing it
/// for a union's merge would be an object read per row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Member {
    pub rev: Rev,
    pub time: i64,
}

impl Member {
    /// The open change is the newest thing there is, so it sorts first
    /// wherever it appears.
    fn open() -> Self {
        Member {
            rev: Rev::Open,
            time: i64::MAX,
        }
    }

    fn commit(&self) -> Option<gix::ObjectId> {
        match self.rev {
            Rev::Open => None,
            Rev::Commit(id) => Some(id.object_id()),
        }
    }
}

/// A set of walk tips — either whatever a sub-plan denotes, or every visible
/// ref, which is the ceiling an unbounded form stops at.
pub(super) enum Tips {
    Of(Box<Plan>),
    Everything,
}

/// The plan IR. Small on purpose: every node here either folds into one gix
/// walk or is a shape that genuinely has to materialize.
pub(super) enum Plan {
    /// Members already in hand, newest first.
    Set(Vec<Member>),
    /// Ancestors of `tips`, minus the ancestors of `hidden`. Lazy.
    Ancestors {
        tips: Tips,
        hidden: Option<Tips>,
    },
    /// Members of the history below `ceiling` that `seeds` reaches forward.
    Descend {
        seeds: Box<Plan>,
        ceiling: Tips,
    },
    Union(Box<Plan>, Box<Plan>),
    /// `keep`, filtered by whether a member is (or is not) in `probe`.
    Filter {
        keep: Box<Plan>,
        probe: Box<Plan>,
        want: bool,
    },
    Latest(Box<Plan>),
    /// `heads(x)` when `heads`, `roots(x)` otherwise.
    Extremes {
        of: Box<Plan>,
        heads: bool,
    },
    /// A predicate over commit content, scanned across visible history.
    Scan {
        field: Field,
        pattern: Pattern,
    },
}

/// A bound expression: the plan, plus the name the resolver used when the
/// whole expression was one revision leaf. The name rides along because
/// recomputing it would mean resolving that leaf a second time.
pub(super) struct Bound {
    pub plan: Plan,
    pub name: Option<String>,
}

/// Stage two. Resolves every leaf, raises every refusal, walks nothing.
pub(super) fn bind(repo: &gix::Repository, expr: &Expr) -> Result<Bound> {
    if let Expr::Revision(token) = expr {
        let leaf = resolve::leaf(repo, token)?;
        return Ok(Bound {
            plan: Plan::Set(vec![member_of(repo, leaf.rev)?]),
            name: leaf.name,
        });
    }
    Ok(Bound {
        plan: plan_of(repo, expr)?,
        name: None,
    })
}

pub(super) fn plan_of(repo: &gix::Repository, expr: &Expr) -> Result<Plan> {
    Ok(match expr {
        Expr::Revision(token) => {
            let leaf = resolve::leaf(repo, token)?;
            Plan::Set(vec![member_of(repo, leaf.rev)?])
        }
        Expr::Complement(inner) => complement(repo, inner)?,
        Expr::Intersection(a, b) => intersection(repo, a, b)?,
        Expr::Union(a, b) => Plan::Union(Box::new(plan_of(repo, a)?), Box::new(plan_of(repo, b)?)),
        // `::x` is a plain ancestor walk; `x::y` and `x::` are the forward
        // forms, which is where the child map gets built.
        Expr::DagRange { from, to } => match (from, to) {
            (None, Some(to)) => Plan::Ancestors {
                tips: Tips::Of(Box::new(plan_of(repo, to)?)),
                hidden: None,
            },
            (Some(from), Some(to)) => Plan::Descend {
                seeds: Box::new(plan_of(repo, from)?),
                ceiling: Tips::Of(Box::new(plan_of(repo, to)?)),
            },
            (Some(from), None) => Plan::Descend {
                seeds: Box::new(plan_of(repo, from)?),
                ceiling: Tips::Everything,
            },
            (None, None) => universe(),
        },
        // `x..y` is literally gix's range. An absent `from` excludes nothing;
        // an absent `to` is every visible head.
        Expr::Range { from, to } => match (from, to) {
            (None, Some(to)) => Plan::Ancestors {
                tips: Tips::Of(Box::new(plan_of(repo, to)?)),
                hidden: None,
            },
            (Some(from), Some(to)) => Plan::Ancestors {
                tips: Tips::Of(Box::new(plan_of(repo, to)?)),
                hidden: Some(Tips::Of(Box::new(plan_of(repo, from)?))),
            },
            (Some(from), None) => Plan::Ancestors {
                tips: Tips::Everything,
                hidden: Some(Tips::Of(Box::new(plan_of(repo, from)?))),
            },
            (None, None) => universe(),
        },
        Expr::Function { name, args } => func::bind(repo, name, args)?,
    })
}

/// Everything the language can see.
fn universe() -> Plan {
    Plan::Ancestors {
        tips: Tips::Everything,
        hidden: None,
    }
}

/// `~x`. The one shape that folds is `~(::b)`, whose complement is exactly
/// what gix's `hidden` computes — hidden paints a tip *and its ancestors*
/// unwanted, which is the ancestor set and nothing else. Every other `x` has
/// to be materialized and subtracted member by member, because hiding `x`
/// there would quietly remove `x`'s ancestors too and answer a question
/// nobody asked.
fn complement(repo: &gix::Repository, inner: &Expr) -> Result<Plan> {
    if let Some(below) = ancestors_of(inner) {
        return Ok(Plan::Ancestors {
            tips: Tips::Everything,
            hidden: Some(Tips::Of(Box::new(plan_of(repo, below)?))),
        });
    }
    Ok(Plan::Filter {
        keep: Box::new(universe()),
        probe: Box::new(plan_of(repo, inner)?),
        want: false,
    })
}

/// `::b` written as itself, for the fold above.
fn ancestors_of(expr: &Expr) -> Option<&Expr> {
    match expr {
        Expr::DagRange {
            from: None,
            to: Some(to),
        } => Some(to),
        _ => None,
    }
}

/// `a & b`. `a & ~(::b)` folds `b` into `a`'s own hidden list and allocates
/// nothing; anything else materializes the cheaper side and filters the
/// other, so the lazy half stays lazy.
fn intersection(repo: &gix::Repository, a: &Expr, b: &Expr) -> Result<Plan> {
    for (walked, complemented) in [(a, b), (b, a)] {
        let Expr::Complement(inner) = complemented else {
            continue;
        };
        let Some(below) = ancestors_of(inner) else {
            continue;
        };
        let hide = Tips::Of(Box::new(plan_of(repo, below)?));
        return Ok(match plan_of(repo, walked)? {
            Plan::Ancestors { tips, hidden: None } => Plan::Ancestors {
                tips,
                hidden: Some(hide),
            },
            keep => Plan::Filter {
                keep: Box::new(keep),
                probe: Box::new(Plan::Ancestors {
                    tips: hide,
                    hidden: None,
                }),
                want: false,
            },
        });
    }

    let left = plan_of(repo, a)?;
    let right = plan_of(repo, b)?;
    // Neither side's size is knowable without running it, so the tie-break is
    // the shape: a leaf is cheaper to hold than a walk, and a walk is cheaper
    // than a scan.
    let (keep, probe) = if cost(&left) < cost(&right) {
        (right, left)
    } else {
        (left, right)
    };
    Ok(Plan::Filter {
        keep: Box::new(keep),
        probe: Box::new(probe),
        want: true,
    })
}

fn cost(plan: &Plan) -> u8 {
    match plan {
        Plan::Set(_) => 0,
        Plan::Latest(_) => 1,
        Plan::Ancestors { .. } => 2,
        Plan::Filter { keep, .. } => cost(keep),
        Plan::Union(a, b) => cost(a).max(cost(b)),
        Plan::Descend { .. } | Plan::Extremes { .. } | Plan::Scan { .. } => 3,
    }
}

/// A resolved leaf as a member. The commit time is read once here so every
/// later ordering decision is arithmetic.
fn member_of(repo: &gix::Repository, rev: Rev) -> Result<Member> {
    Ok(match rev {
        Rev::Open => Member::open(),
        Rev::Commit(id) => Member {
            rev,
            time: commit_time(repo, id.object_id())?,
        },
    })
}

fn commit_time(repo: &gix::Repository, id: gix::ObjectId) -> Result<i64> {
    Ok(repo
        .find_commit(id)
        .map_err(Error::repo)?
        .time()
        .map_err(Error::repo)?
        .seconds)
}

/// Stage three. Every arm returns the same iterator type, so the caller never
/// learns whether it got a walk, a vector, or a filter over either.
pub(super) fn run<'r>(
    repo: &'r gix::Repository,
    plan: Plan,
) -> Result<Box<dyn Iterator<Item = Result<Member>> + 'r>> {
    match plan {
        Plan::Set(members) => Ok(Box::new(members.into_iter().map(Ok))),
        Plan::Ancestors { tips, hidden } => {
            let (tip_ids, open) = tip_ids(repo, tips)?;
            let hidden_ids = match hidden {
                Some(tips) => tip_ids_only(repo, tips)?,
                None => Vec::new(),
            };
            let walk = walk_from(repo, tip_ids, hidden_ids)?;
            // The open change is not a tip anyone can hand gix: a walk rooted
            // at `@` is rooted at HEAD's commit, with the open change riding
            // in front of it.
            Ok(match open {
                true => Box::new(std::iter::once(Ok(Member::open())).chain(walk)),
                false => walk,
            })
        }
        Plan::Descend { seeds, ceiling } => {
            let seeds = materialize(repo, *seeds)?;
            let seed_ids: HashSet<gix::ObjectId> =
                seeds.iter().filter_map(Member::commit).collect();
            let open = seeds.iter().any(|m| m.rev == Rev::Open);
            let ceiling = tip_ids_only(repo, ceiling)?;
            let graph = Graph::build(repo, ceiling, None)?;

            // Parents are emitted after their children, so one reverse pass
            // is the whole forward propagation — no second sort, and no child
            // edges asked of git, which has none to give.
            let mut reached = vec![false; graph.order.len()];
            for i in (0..graph.order.len()).rev() {
                reached[i] = seed_ids.contains(&graph.order[i])
                    || graph.parents[i].iter().any(|&p| reached[p]);
            }
            let mut out: Vec<Member> = (0..graph.order.len())
                .filter(|&i| reached[i])
                .map(|i| graph.member(i))
                .collect();
            if open {
                out.insert(0, Member::open());
            }
            Ok(Box::new(out.into_iter().map(Ok)))
        }
        Plan::Union(a, b) => Ok(Box::new(Merge {
            left: run(repo, *a)?.peekable(),
            right: run(repo, *b)?.peekable(),
            seen: HashSet::new(),
        })),
        Plan::Filter { keep, probe, want } => {
            let probe: HashSet<Rev> = materialize(repo, *probe)?
                .into_iter()
                .map(|m| m.rev)
                .collect();
            let base = run(repo, *keep)?;
            Ok(Box::new(base.filter(move |m| match m {
                Ok(m) => probe.contains(&m.rev) == want,
                Err(_) => true,
            })))
        }
        Plan::Latest(of) => Ok(Box::new(run(repo, *of)?.take(1))),
        Plan::Extremes { of, heads } => {
            let members = materialize(repo, *of)?;
            Ok(Box::new(
                extremes(repo, members, heads)?.into_iter().map(Ok),
            ))
        }
        Plan::Scan { field, pattern } => {
            let tips = resolve::universe_tips(repo)?;
            let base = walk_from(repo, tips, Vec::new())?;
            Ok(Box::new(base.filter_map(move |m| match m {
                Err(err) => Some(Err(err)),
                Ok(m) => match matches(repo, &m, field, &pattern) {
                    Ok(true) => Some(Ok(m)),
                    Ok(false) => None,
                    Err(err) => Some(Err(err)),
                },
            })))
        }
    }
}

/// The one place a walk is constructed.
fn walk_from<'r>(
    repo: &'r gix::Repository,
    tips: Vec<gix::ObjectId>,
    hidden: Vec<gix::ObjectId>,
) -> Result<Box<dyn Iterator<Item = Result<Member>> + 'r>> {
    if tips.is_empty() {
        return Ok(Box::new(std::iter::empty()));
    }
    let mut platform = repo
        .rev_walk(tips)
        .sorting(Sorting::ByCommitTime(CommitTimeOrder::NewestFirst));
    if !hidden.is_empty() {
        platform = platform.with_hidden(hidden);
    }
    let walk = platform.all().map_err(Error::repo)?;
    Ok(Box::new(walk.map(|info| {
        let info = info.map_err(Error::repo)?;
        Ok(Member {
            rev: Rev::Commit(CommitId::new(info.id)),
            time: info.commit_time.unwrap_or_default(),
        })
    })))
}

fn materialize(repo: &gix::Repository, plan: Plan) -> Result<Vec<Member>> {
    run(repo, plan)?.collect()
}

/// Tip ids for a walk, plus whether the open change was among them. The open
/// change has no id, so it contributes HEAD's commit — the commit it sits on,
/// which is what git already calls it.
fn tip_ids(repo: &gix::Repository, tips: Tips) -> Result<(Vec<gix::ObjectId>, bool)> {
    match tips {
        Tips::Everything => Ok((resolve::universe_tips(repo)?, false)),
        Tips::Of(plan) => {
            let members = materialize(repo, *plan)?;
            let open = members.iter().any(|m| m.rev == Rev::Open);
            let mut ids: Vec<gix::ObjectId> = members.iter().filter_map(Member::commit).collect();
            if open
                && let Some(head) = resolve::open_commit(repo)?
                && !ids.contains(&head)
            {
                ids.push(head);
            }
            Ok((ids, open))
        }
    }
}

fn tip_ids_only(repo: &gix::Repository, tips: Tips) -> Result<Vec<gix::ObjectId>> {
    Ok(tip_ids(repo, tips)?.0)
}

/// `heads(x)` and `roots(x)`, from one pass over the history the set spans.
///
/// The cutoff at the oldest member is what keeps this from being a walk of
/// everything: no member is older than that, so no path *between* members
/// leaves the window either.
fn extremes(repo: &gix::Repository, members: Vec<Member>, heads: bool) -> Result<Vec<Member>> {
    let ids: Vec<gix::ObjectId> = members.iter().filter_map(Member::commit).collect();
    let open = members.iter().any(|m| m.rev == Rev::Open);
    let cutoff = members
        .iter()
        .filter(|m| m.rev != Rev::Open)
        .map(|m| m.time)
        .min();
    let graph = Graph::build(repo, ids.clone(), cutoff)?;
    let set: HashSet<gix::ObjectId> = ids.into_iter().collect();
    let n = graph.order.len();
    let member: Vec<bool> = (0..n).map(|i| set.contains(&graph.order[i])).collect();

    let mut flagged = vec![false; n];
    if heads {
        // A child is emitted before its parents, so one forward pass carries
        // "a member descends from me" up the ancestry.
        for i in 0..n {
            if member[i] || flagged[i] {
                for &p in &graph.parents[i] {
                    flagged[p] = true;
                }
            }
        }
    } else {
        for i in (0..n).rev() {
            flagged[i] = graph.parents[i].iter().any(|&p| member[p] || flagged[p]);
        }
    }

    let mut out: Vec<Member> = (0..n)
        .filter(|&i| member[i] && !flagged[i])
        .map(|i| graph.member(i))
        .collect();
    if open {
        // Nothing descends from the open change, so it is always a head. It
        // is a root only when the commit it sits on brings no member with it.
        let dominated = !heads
            && match resolve::open_commit(repo)? {
                Some(head) => match graph.index.get(&head) {
                    Some(&i) => member[i] || flagged[i],
                    None => false,
                },
                None => false,
            };
        if !dominated {
            out.insert(0, Member::open());
        }
    }
    Ok(out)
}

fn matches(
    repo: &gix::Repository,
    member: &Member,
    field: Field,
    pattern: &Pattern,
) -> Result<bool> {
    let Some(id) = member.commit() else {
        return Ok(false);
    };
    let commit = repo.find_commit(id).map_err(Error::repo)?;
    Ok(match field {
        Field::Description => {
            pattern.matches(&commit.message_raw().map_err(Error::repo)?.to_string())
        }
        Field::Author => {
            let author = commit.author().map_err(Error::repo)?;
            pattern.matches(&author.name.to_string()) || pattern.matches(&author.email.to_string())
        }
    })
}

/// The commit graph of one region of history, in emission order.
///
/// Emission order is the load-bearing property: gix queues a parent only once
/// its child has been popped, so every child precedes every parent in this
/// vector regardless of what the clocks say. One forward pass propagates
/// toward the roots and one reverse pass propagates toward the heads, and
/// neither needs a topological sort of its own.
struct Graph {
    order: Vec<gix::ObjectId>,
    time: Vec<i64>,
    parents: Vec<Vec<usize>>,
    index: HashMap<gix::ObjectId, usize>,
}

impl Graph {
    fn build(
        repo: &gix::Repository,
        tips: Vec<gix::ObjectId>,
        cutoff: Option<i64>,
    ) -> Result<Graph> {
        let mut graph = Graph {
            order: Vec::new(),
            time: Vec::new(),
            parents: Vec::new(),
            index: HashMap::new(),
        };
        if tips.is_empty() {
            return Ok(graph);
        }
        let sorting = match cutoff {
            Some(seconds) => Sorting::ByCommitTimeCutoff {
                order: CommitTimeOrder::NewestFirst,
                seconds,
            },
            None => Sorting::ByCommitTime(CommitTimeOrder::NewestFirst),
        };
        let mut raw: Vec<Vec<gix::ObjectId>> = Vec::new();
        let walk = repo
            .rev_walk(tips)
            .sorting(sorting)
            .all()
            .map_err(Error::repo)?;
        for info in walk {
            let info = info.map_err(Error::repo)?;
            graph.index.insert(info.id, graph.order.len());
            graph.order.push(info.id);
            graph.time.push(info.commit_time.unwrap_or_default());
            raw.push(info.parent_ids.iter().copied().collect());
        }
        graph.parents = raw
            .iter()
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| graph.index.get(id).copied())
                    .collect()
            })
            .collect();
        Ok(graph)
    }

    fn member(&self, i: usize) -> Member {
        Member {
            rev: Rev::Commit(CommitId::new(self.order[i])),
            time: self.time[i],
        }
    }
}

/// A union, merged on commit time. Both sides already arrive newest first, so
/// this is a two-way merge with a seen set — which is also what makes a union
/// lazy: taking twenty-five rows pulls at most twenty-five from each side.
struct Merge<'r> {
    left: std::iter::Peekable<Box<dyn Iterator<Item = Result<Member>> + 'r>>,
    right: std::iter::Peekable<Box<dyn Iterator<Item = Result<Member>> + 'r>>,
    seen: HashSet<Rev>,
}

impl Iterator for Merge<'_> {
    type Item = Result<Member>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let take_left = match (self.left.peek(), self.right.peek()) {
                (None, None) => return None,
                // An error on either side is the answer; peeking told us it
                // is there, so take that side and hand it up.
                (Some(Err(_)), _) => true,
                (_, Some(Err(_))) => false,
                (Some(Ok(l)), Some(Ok(r))) => l.time >= r.time,
                (Some(Ok(_)), None) => true,
                (None, Some(Ok(_))) => false,
            };
            let next = if take_left {
                self.left.next()
            } else {
                self.right.next()
            };
            match next {
                Some(Ok(member)) => {
                    if self.seen.insert(member.rev) {
                        return Some(Ok(member));
                    }
                }
                other => return other,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use ff_testsupport::Fixture;

    use super::*;
    use crate::revset::parse::parse;

    /// A history deep enough that walking it would be visible, so a refusal
    /// that arrives anyway proves the refusal did not walk.
    fn deep() -> Fixture {
        let fx = Fixture::new();
        for i in 0..200 {
            fx.write("a.txt", &format!("{i}\n"));
            fx.commit(&format!("commit {i}"));
        }
        fx
    }

    /// The hard requirement, asserted by structure rather than by a clock: a
    /// timing assertion here would be a flake generator, so instead the test
    /// names the stage. `bind` resolves leaves and returns; it has no arm
    /// that constructs a walk, and every refusal below comes out of it.
    #[test]
    fn no_error_in_the_language_costs_a_walk() {
        let fx = deep();
        let repo = fx.repo();
        for src in [
            "nosuchbranch",
            "@^",
            "@~2",
            "main^!",
            "nosuchbranch..main",
            "::nosuchbranch",
            "latest(nosuchbranch)",
            "nosuch(main)",
            "latest(main, main)",
            "description(regex:x)",
            "base(main)",
        ] {
            let expr = parse(src).expect("parses");
            let err = match bind(&repo, &expr) {
                Err(err) => err,
                Ok(_) => panic!("{src} must be refused at bind time"),
            };
            assert!(
                err.id().starts_with("usage/") || err.id().starts_with("revset/"),
                "{src} raised {}",
                err.id()
            );
        }
    }

    /// The other half of the same claim: a *good* revset over the same deep
    /// history binds to a plan that has walked nothing yet.
    #[test]
    fn binding_a_deep_revset_materializes_nothing() {
        let fx = deep();
        let repo = fx.repo();
        let bound = bind(&repo, &parse("::main").expect("parses")).expect("binds");
        assert!(
            matches!(bound.plan, Plan::Ancestors { .. }),
            "`::main` must still be an unevaluated walk after binding"
        );

        // And taking a handful of rows off it costs a handful of pops.
        let taken: Vec<_> = run(&repo, bound.plan)
            .expect("runs")
            .take(5)
            .collect::<Result<Vec<_>>>()
            .expect("rows");
        assert_eq!(taken.len(), 5);
    }
}
