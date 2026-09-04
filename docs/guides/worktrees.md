# Worktrees

[`ff switch`](../reference/cli/switch.md) already covers most parallel work: the open change — the uncommitted work in your tree — parks with the branch you leave and resumes when you come back, so one tree serves many branches.

A worktree is for the moments when one tree is the bottleneck — a long build or test run that should keep going while you edit something else, an agent working a branch alongside you, a review checkout you keep standing.

`ff worktree add` makes a second checkout of the same repository: one object store and one set of branches are shared, and the working tree, the index, HEAD, and the operation log are the worktree's own.

Every transcript below is real `ff` output. fufu calls a secondary worktree a bay in places, and this page does too.

## A second checkout

Bare [`ff worktree`](../reference/cli/worktree.md) is the list. A fresh clone has one row — the tree you are standing in, marked `*`, with its checkout path and the branch it stands on:

```console
$ ff worktree
* main      /tmp/tmp.2gL2qhGWtM/demo  main
```

`ff worktree add <path>` makes the bay. The branch is a name you give, or a new branch named after the directory when you do not say, or a minted name when that name is already taken:

```console
$ ff worktree add ../bay
made bay at /tmp/tmp.2gL2qhGWtM/bay on bay
  on a new branch
  its log is refs/fufu/wt/bay/ops

$ ff worktree
* main      /tmp/tmp.2gL2qhGWtM/demo  main
  bay       /tmp/tmp.2gL2qhGWtM/bay   bay
```

The `its log` line is the part git does not have: each worktree carries its own operation chain, and the chain floor is laid as the worktree is made, so [`ff undo`](../reference/cli/undo.md) works in the bay from its first command. A checkout written by hand with `git worktree add` gets its floor on its first fufu command instead, and undo in it is blind until then.

## Each tree has its own open change

Every worktree holds exactly one [open change](../concepts/changes.md). [`ff status`](../reference/cli/status.md) in the bay is about the bay — its own uncommitted files, on its own branch, with capture running there the same as anywhere:

```console
$ ff status
on bay · nothing to sync
@  ntnvpxxu 5324a259   0s ago
│  (no description)
│  A src/lexer.rs +1  -0  ++++++++++++++++++++
│    1 file       +1  -0
●  —        dd510982   0s ago
│  release: cut v0.1.0

$ ff commit -m "lexer: sketch the tokenizer"
closed 9b602cf2 on bay: lexer: sketch the tokenizer (1 file(s))
undo: ff undo
```

Meanwhile the first tree keeps its own change moving, on its own branch, with no coordination between the two:

```console
$ ff commit -m "docs: say what this is"
closed 9a407e42 on main: docs: say what this is (1 file(s))
undo: ff undo
```

## One repository, a log per tree

The commits land in one shared repository, so what does `ff undo` mean when two trees are writing to it? The answer is scoping: an operation belongs to the chain of the worktree that ran it, and `ff undo` steps back the chain of the tree you run it in. Close another change in the bay and undo it there:

```console
$ ff commit -m "lexer: emit spans"
closed 27cbe3ef on bay: lexer: emit spans (1 file(s))
undo: ff undo

$ ff undo
undid: commit on bay: lexer: emit spans
  now at vouzzkuzwxmp (pre: ff commit -m lexer: emit spans)
  refs/heads/bay → 9b602cf2
back: ff redo
```

The commit on `main` stands untouched, because it was never on this chain. [`ff history`](../reference/cli/history.md) in each tree shows the split — the bay's chain holds the bay's operations:

```console
$ ff history
↑1  tzrnvtuv    0s ago  redo  commit on bay: lexer: emit spans
@   vouzzkuz    0s ago  now   pre: ff commit -m lexer: emit spans
↓1  vxvvznln    0s ago  undo  commit on bay: lexer: sketch the tokenizer
↓2  ntnvpxxu    0s ago  undo  pre: ff status
↓3  nwrykqmu    0s ago  undo  operation log initialized from observed state; earlier operations not undoable
    (the floor)
```

and the first tree's chain holds its own:

```console
$ ff history
@   uqvtonqz    0s ago  now   commit on main: docs: say what this is
↓1  mzsmxzko    0s ago  undo  pre: ff commit -m docs: say what this is
↓2  xmvvnypu    0s ago  undo  add worktree bay on bay
↓3  uptkprtt    0s ago  undo  operation log initialized from observed state; earlier operations not undoable
    (the floor)
```

Note the `add worktree bay on bay` row: making the bay was itself an operation, on the chain of the tree that ran it, so an `ff undo` right after it would have taken the checkout away again. [Snapshots and undo](../concepts/snapshots-and-undo.md) covers what each press of undo restores.

## Two writers, one repository

Two trees writing one repository is this guide's normal case — you in one, a build or an agent in the other — so the locking is worth saying plainly. Each chain has one write lock, a file at `<common-dir>/fufu/oplog-<chain>.lock`, and no lock spans the repository: a verb in the bay and a verb in the first tree write two different chains and never wait on each other.

The commits and refs underneath stay safe by git's own rules, the same as any two git processes sharing a repository.

### Two processes in one tree

Inside one tree, two fufu processes at once — an agent's hook capturing while you type a verb — settle at that chain's one lock, and the loser loses cleanly. A capture that finds the lock held is skipped outright, because another process is already recording and the next capture is moments away.

A verb waits up to two seconds, then refuses with `ref/contended: another fufu process is writing the operation log` rather than writing over what the other holds — run it again. That refusal exits 4, the one code that means exactly that; [the error id index](../reference/errors.md) has the rest. Neither case can corrupt the log.

[Architecture](../internals/architecture.md#where-fufus-state-lives) places the lock file among the rest of fufu's on-disk state.

## Watching every tree

[`ff watch`](../reference/cli/watch.md) streams the operation log as it moves, one JSON object per line. Bare, it streams the chain of the worktree you run it in; `--all` streams every chain in the repository, opening with one `start` event per worktree, and every line names the worktree it belongs to. Land another commit in the bay while a stream opened from the first tree is running:

```console
$ ff commit -m "lexer: spans and byte offsets"
closed 0741da9f on bay: lexer: spans and byte offsets (1 file(s))
undo: ff undo
```

The stream saw the whole thing — the two opening events, then the bay's pre-commit capture, then the commit. `-n 4` stops it after four events, counting the opening ones:

```console
$ ff watch --all -n 4
{"ff":1,"cmd":"watch","data":{"worktree":"bay","motion":"start","tip":"vouzzkuzwxmpkxuplnmpwrqxwlzlrtzqmvotqvvv"}}
{"ff":1,"cmd":"watch","data":{"worktree":"main","motion":"start","tip":"uqvtonqznmnvsuollmpllntopvozymosluqtwnmq"}}
{"ff":1,"cmd":"watch","data":{"worktree":"bay","motion":"landed","op":{"id":"kqzuwvlqrumvlqsmprzmvlsxztzmsrxwqktywqvo","short_id":"kqzu","kind":"capture","verb":"","summary":"pre: ff commit -m lexer: spans and byte offsets","time":1787986478,"branch":"bay","session":null,"undo_of":null}}}
{"ff":1,"cmd":"watch","data":{"worktree":"bay","motion":"landed","op":{"id":"ntnnpsyrsuoywzuovkpqmzzzyloloklqvnolmvvp","short_id":"ntnn","kind":"op","verb":"commit","summary":"commit on bay: lexer: spans and byte offsets","time":1787986478,"branch":"bay","session":null,"undo_of":null}}}
```

That capture event is the point for anyone supervising a bay from outside it: capture runs in a secondary worktree exactly as in the first, and a watcher in any tree sees it happen. `--kind` narrows the stream to captures or verbs, `--session` follows one agent's motion, and a stream under `--all` keeps a bay's chain even after the worktree is removed; the [watch reference](../reference/cli/watch.md) has the full event grammar.

## Parking crosses trees with its branch

A parked change belongs to its branch, and the branch lives in the shared ref namespace, so any tree can resume it. Start a line of work in the first tree, edit, and park it by switching away:

```console
$ ff start
minted ff/nimble-badger (forked from main)
open change on ff/nimble-badger
undo: ff undo

$ ff switch main
parked the open change on ff/nimble-badger (87fd8ffd)
switched to main
undo: ff undo
```

Now take that branch from the bay. The parked change resumes there — same files, same edits, same pending description:

```console
$ ff switch ff/nimble-badger
ff: absorbed changes made outside fufu:
  refs/fufu/parked/ff/nimble-badger created at 87fd8ffd (park: wip on ff/nimble-badger)
  refs/heads/ff/nimble-badger created at dd510982 (branch: forked from main)
  refs/stash created at 87fd8ffd (On ff/nimble-badger: fufu: wip on ff/nimble-badger)
switched to ff/nimble-badger
resumed the parked change (1 file(s))
undo: ff undo
```

The absorbed lines are the two chains staying honest with each other. Each worktree's chain keeps its own record of the repository's refs, and the parking refs the first tree wrote are new to the bay's chain, so its next verb absorbs them out loud before acting — [the two regimes](../concepts/two-regimes.md) explains why motion a chain did not perform is never silently blended in.

While the branch is open in the bay, the first tree cannot take it. git allows one branch in two checkouts behind a flag; fufu refuses outright:

```console
$ ff switch ff/nimble-badger
ff: 'ff/nimble-badger' is already used by worktree at '/tmp/tmp.2gL2qhGWtM/bay'
  try:
    ff worktree list
    git worktree list
```

Switching the bay back to its own branch parks the change again, with its branch, where any tree can pick it up later:

```console
$ ff switch bay
parked the open change on ff/nimble-badger (87fd8ffd)
switched to bay
undo: ff undo
```

## Removal captures first

The bay now holds a half-written, uncommitted file. `git worktree remove` demands `--force` for a dirty tree because it has nowhere to put the work. [`ff worktree remove`](../reference/cli/worktree-remove.md) has no `--force`, because the capture comes first, into the bay's own chain, and the removal says where the work went:

```console
$ ff worktree remove bay
removed bay (was on bay)
  captured first as xkkmurzrlwos — ff restore <path> --at-op xkkmurzrlwos
  its log stays at refs/fufu/wt/bay/ops
```

The chain outlives the checkout. [`ff worktree list`](../reference/cli/worktree-list.md) shows it under the gone chains, with the capture's operation id on the row:

```console
$ ff worktree list
* main      /tmp/tmp.2gL2qhGWtM/demo  main

chains whose worktree is gone
  bay       bay  xkkmurzrlwos  0s ago
ff restore <path> --at-op <op>  brings a file back from one
```

That id is an address. [`ff restore`](../reference/cli/restore.md) with `--at-op` brings a file out of the capture into whatever tree you are standing in, where it joins the open change like any other edit:

```console
$ ff restore src/lexer_test.rs --at-op xkkmurzrlwos
restored from xkkm (pre: ff worktree remove bay)
  restored  src/lexer_test.rs
undo: ff undo

$ ff status
on main · 1 to publish
@  noxokvry ab68e949   0s ago
│  (no description)
│  A src/lexer_test.rs +1  -0  ++++++++++++++++++++
│    1 file            +1  -0
●  mzsmxzko 9a407e42   0s ago
│  docs: say what this is
```

The removal is one operation on the chain of the tree that ran it, so `ff undo` right after it puts the whole checkout back, uncommitted work included. Two limits to know:

- Ignored files — build outputs, `node_modules`, virtualenvs — are not captured and do not come back, the same trade any worktree removal makes.
- Gone chains age out on the ordinary `fufu.keep` retention window (90 days by default), so commit or restore what matters before [`ff trim`](../reference/cli/trim.md) gets there.

## From here

- [`ff worktree add`](../reference/cli/worktree-add.md), [`list`](../reference/cli/worktree-list.md), [`remove`](../reference/cli/worktree-remove.md), and [`ff watch`](../reference/cli/watch.md) — the reference for every flag.
- [Changes](../concepts/changes.md) and [branches](../concepts/branches.md) — the model behind parking and resuming.
- [Snapshots and undo](../concepts/snapshots-and-undo.md) — what a chain holds and what each press of undo restores.
