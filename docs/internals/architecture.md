# Architecture

fufu has three main layers: capture and recovery records, replay prediction, and operations that update Git state. This contributor guide maps those layers to code and storage. Start with [Changes](../concepts/changes.md) for the user model; the [founding design](design.md) is historical and includes unshipped proposals.

```text
ff-cli: command parsing, preflight lanes, transport, rendering, integrations
   │
   ├─ ff-core: snapshot/ + ops/       capture and recovery
   ├─ ff-core: futures.rs            replay predictions
   └─ ff-core: rewrite.rs + verbs    planned local transitions
                  │
           gix objects, refs, index, trees
```

<span id="floor-1-capture"></span>

## Layer 1 — capture and recovery

`crates/ff-cli/src/cli.rs` declares each command's preflight behavior; `lanes.rs` runs capture, fetch, and maintenance where enabled. Repository readers normally attempt capture, while mutators capture after initial guards in core verb setup. [`ff git`](../reference/cli/git.md) checks policy before attempting capture and invoking Git.

Shell and agent adapters call [`ff trigger`](../reference/cli/trigger.md) at the events their [installed hooks](../reference/hooks/index.md) support. These are capture opportunities, not continuous filesystem observation. Skipped files, contention, missing identity, inactive hooks, and retention affect [recovery coverage](../concepts/snapshots-and-undo.md#coverage-and-limits).

### The operation-log commit

`ops/append.rs` writes operations; `ops/message.rs` encodes their trailers; `ops/walk.rs` decodes them. Each worktree has a chain at `refs/fufu/wt/<id>/ops`.

| Component | Meaning |
| --- | --- |
| Operation tree | Recorded working-copy tree, or the planned result of a write-ahead verb. Capture exclusions apply. |
| `fufu-prev` | Previous operation; its parent edge is present only when a predecessor exists. The initial operation has no predecessor. |
| `fufu-base` | HEAD's commit at recording, when one exists; a parent edge keeps it reachable. |
| Record commit | Non-capture operations carry `op.json`, a ref table, and an index tree where applicable, with extra parents pinning transition objects. |
| `fufu-open` | Internal open commit, or no open commit; its parent edge pins the object. Older records may lack this trailer. |
| Skip links | `fufu-prev-branch`, `fufu-prev-segment`, and `fufu-prev-verb` support branch-local and filtered walks. Missing legacy links differ from an explicit `none`. |

A capture inherits its predecessor's tracked-ref table and has no separate record commit. It still updates fufu's chain, branch snapshot pointer, and open ref. “Capture changes no refs” in older design prose meant no tracked user-ref transitions, not no ref writes.

`ops/verb.rs` records planned local transitions before applying them. Records describe refs, tree, index, and HEAD so recovery can restore state rather than rerun commands. Push records are written after the remote update; undo cannot reverse that update. Initial guards, object writes, and metadata updates mean a refusal is not a universal no-write guarantee.

[`ff undo`](../reference/cli/undo.md) moves the worktree's chain pointer rather than appending a navigation operation. Reflogs retain paths left behind. The repository's `gc` configuration protects fufu reflogs from automatic expiration until [retention](../guides/recovery.md#retention-and-the-earliest-recovery-point) deliberately releases history. Other live worktrees have their own chains and restoration constraints.

<span id="absorption"></span>

### Reconciliation after outside changes

Verb setup compares the remembered ref state with the repository and records observed outside changes as a foreign operation before capture. Readers can also reconcile through their preflight. One gap becomes one foreign operation; Git reflogs cannot supply intermediate working-copy bytes. [Using fufu alongside Git](../concepts/two-regimes.md#returning-after-outside-changes) owns the user-facing explanation.

<span id="floor-2-futures"></span>

## Layer 2 — replay predictions

`futures::probe` reapplies a commit range onto a moving cursor through in-memory three-way tree merges. For the current branch it also considers the open change as a final step, so a prediction can report a conflict in uncommitted work.

The probe uses a memory-backed object store and discards the temporary merge objects. That pure computation is distinct from the CLI command around it: capture, cache writes, fetching, and maintenance can change disk state.

### The verdict set

The base-axis verdicts include up-to-date, fast-forward, clean replay, and conflict. Unsupported or over-budget cases report unknown, including unrelated histories, merges in a replay range, and the `fufu.futuresDepth` cap (200 by default). An explicitly requested replay is not limited by the prediction cap.

Remote reporting also distinguishes a missing copy, never-published work, and local rollback relative to the last published tip. Base and remote are separate axes using the same replay machinery. See [pulling and pushing](../concepts/push-boundary.md) for their user-facing decisions.

### The cache

Each branch's file at `<common-dir>/fufu/futures/<branch>` contains a slot per axis. The key includes the reference being compared, its tip, the local branch tip, and the open-change tree. A changed input invalidates that slot; deleting the file causes recomputation. This is a pure cache.

### Where the answer is spent

[`ff status`](../reference/cli/status.md) displays replay predictions. The bare map and [`ff branch`](../reference/cli/branch.md) avoid running a merge simulation for every displayed branch; their cheaper counts and labels do not prove a replay will succeed.

<span id="floor-3-the-verbs"></span>

## Layer 3 — local operations

Per-verb modules build and apply transitions using shared capture, index, ref, and rewrite code. Branch creation and editing sessions keep HEAD attached. Held rewrites record a pending request against ordinary Git inputs rather than advancing the affected branch to a logical conflicted commit.

A conflicting branch can hold after other branches in a cascade have landed. The [cascade](../concepts/branches.md#the-cascade) documents skips, and [conflict reports](../concepts/held-rewrites.md#reading-conflict-reports) document per-verb exit distinctions. [`ff resolve`](../reference/cli/resolve.md) and [`ff done`](../reference/cli/done.md) apply user resolutions through the same rewrite machinery.

### The rewrite engine

`rewrite.rs` handles commit rewriting. Rewording reparents descendants without replaying trees. Content-changing rewrites perform three-way merges and record old-to-new mappings in the operation.

- Replays drop and report non-root, non-merge commits that become empty; reword-only operations do not apply that rule.
- Replays can also drop changes already represented by a surviving change ID in the base, reporting them as superseded.
- Surviving changes keep their change IDs, while rewritten commit SHAs change. [Revisions and IDs](../reference/revisions.md) owns addressing and lookup limits.
- [`ff restack`](../reference/cli/restack.md) replays onto a base; [`ff pull`](../reference/cli/pull.md) adds remote reconciliation and fetching. [`ff push`](../reference/cli/push.md) remains an explicit send, callable by a person or script.

## Where fufu's state lives

Repository records live in shared Git refs and under `<common-dir>/fufu/`. For the main checkout the common directory is normally `.git`; linked worktrees share it. User-level hook configuration and the update cache live outside this layout.

| Ref | What it holds |
| --- | --- |
| `refs/fufu/wt/<id>/ops` | A worktree's operation chain. The main ID is `main`; a linked worktree uses its Git admin-directory basename. |
| `refs/fufu/wt/<id>/trash/@ops` | That chain's pre-trim tip, retained for recovery of the last trim. |
| `refs/fufu/snap/<branch>` | The newest operation recorded on the branch. |
| `refs/fufu/open/<branch>` | The internal open commit, including parked work. A clean state removes the ref; retained operations can still pin previous versions. Committing may reuse or replace this object. |
| `refs/fufu/seen/<branch>` | The remote tip recorded by a reporting pull, successful push, remote-branch creation through switch, or clone. Fetch alone and pull dry-run do not advance it. |
| `refs/fufu/published/<branch>` | The tip this repository last successfully sent for that branch. A pull may change seen without changing published. |
| `refs/fufu/trash/<branch>` | A deleted branch's snapshot timeline pointer; operation records retain its branch-tip transition. |
| `refs/fufu/parked/<branch>` | Legacy stash-based park, converted to an open commit on arrival. Current parking does not create these. |
| `refs/fufu/legacy/*` | Older snapshot/journal chains retained during migration. |

Seen and published refs are outside the tracked-ref restoration set: undo does not un-see or un-send a remote tip. `seen.rs`, `published.rs`, and `push.rs` implement that distinction and the [push lease](../concepts/push-boundary.md#push-carries-a-lease).

Chains use the shared namespace so Git garbage collection can reach retained objects from the main repository and so a chain can outlive a removed worktree. Recovery after removal still depends on capture and retention; see [worktree recovery](../guides/worktrees.md).

| Path | Role |
| --- | --- |
| `<common-dir>/fufu/futures/<branch>` | Rebuildable replay-prediction cache. |
| `<common-dir>/fufu/branch/<branch>` | Pending message, recorded base/fork point, edit session, held rewrite, and resolution metadata. Empty metadata removes the file. |
| `<common-dir>/fufu/ops/<chain>/live`, `…/trash` | Sorted operation-ID indexes derived from retained chains. |
| `<common-dir>/fufu/oplog-<chain>.lock` | Serializes writes to a worktree's operation chain. |

### Caches and retained records

Futures caches and operation-ID indexes can be rebuilt from their inputs. Operation chains, open refs, seen/published records, and branch metadata retain information that the current branch tips alone cannot reconstruct. Deleting them can lose recovery history, pending requests, and access to parked work; once the last reference is gone, Git may garbage-collect the objects.

Removing the fufu executable leaves these records and ordinary Git history in place. Returning after outside Git changes invokes reconciliation, not reconstruction of deleted records. [Leaving and coming back](../concepts/two-regimes.md#leaving-and-coming-back) covers the practical procedure. Only fufu should write its internal refs and metadata.
