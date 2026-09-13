# The machine surface

**Scripts, CI jobs, and agents can read `--json` reports, branch on structured error IDs, and check exit codes. Pin and test the binary versions they support.**

A verb computes one data model, and the human rendering and the JSON rendering are both readers of it. Neither is a translation of the other.

So `--json` is not the human layout re-serialized. [`ff status`](../reference/cli/status.md) crops to what an eye wants, while its JSON carries the model whole: the full change list, the [open change](../concepts/changes.md), the parent commit, the pull futures.

That is what keeps the two from drifting apart, and it is why a script should parse the JSON and never the display text.

> The status, log, evolog, history, and op JSON examples come from `scripts/docs/machine-surface-transcript.sh` in a scratch repository. `jq .` expands the single-line output for reading. IDs, timestamps, and paths vary between runs.

## The envelope

Every `--json` emission is one JSON object on one line, wrapped in a versioned envelope:

```console
$ ff version --json
{"ff":1,"cmd":"version","data":{"version":"0.11.0","commit":"677b97a","date":"2026-09-02","update":{"status":"unofficial","latest":null}}}
```

`ff` is the contract version, currently `1`. `cmd` names the verb that answered. The payload is `data` on success and `error` on failure, never both:

```console
$ ff show doesnotexist --json
{"ff":1,"cmd":"show","error":{"id":"usage/revset-unknown-revision","message":"no revision here answers to `doesnotexist`","exits":["ff log","ff branch"]}}
$ echo $?
2
```

`error.id` is the machine name to branch on, rather than the message prose. IDs have been renamed or removed across releases, so match against the supported binary version. `error.exits` carries suggested next commands as data.

[`ff explain <id>`](../reference/cli/explain.md) turns any id back into prose on demand. [The error id index](../reference/errors.md) lists every id with its exit code, and `ff explain --list` prints the same table from the binary.

### Compatibility in current releases

The envelope currently uses `ff: 1`, but that number alone does not establish payload compatibility across releases. Releases have renamed or removed fields and error IDs without changing it: v0.13.0 renamed `sync/*` and `publish/*` errors, v0.14.0 removed `id_letters`, and v0.15.0 renamed arrival fields and removed operation `route`.

Pin and test the fufu version your script supports, assert the envelope version, read fields by name, and tolerate unknown fields. Check [the changelog](../changelog.md) when upgrading. A stronger cross-release payload or error-ID stability guarantee requires a separately established versioning policy.

The human rendering promises none of this. Layout, wording, and color are free to change in any release.

Timestamps are unix seconds, always named `time`. Commit ids and operation ids are hex, forty characters in JSON, and the slot decides which a hex prefix means — a revision slot reads a sha or a change id, an operation slot an operation id. A `change_id` is a commit's identity across rewrites, spelled in the letters k–z and never hex: the letters column the human views print. See [Revisions and IDs](../reference/revisions.md) for accepted expressions, prefix limits, and past-state reads, and [Snapshots and undo](../concepts/snapshots-and-undo.md) for recovery scope.

## `ff status --json`

The whole working-copy model in one read: where you are, what changed, the open change, its parent, conflicts, foreign drift, and what a pull would do.

```console
$ ff status --json | jq .
{
  "ff": 1,
  "cmd": "status",
  "data": {
    "head": {
      "state": "branch",
      "name": "main",
      "ref": "refs/heads/main",
      "commit": "e629c0d9955c9184bb66538b09463db90f77c9f2"
    },
    "root": "/tmp/tmp.q3ZBT5iKNP",
    "worktree": {
      "id": "main",
      "linked": false,
      "main": "/tmp/tmp.q3ZBT5iKNP"
    },
    "base": null,
    "remote": null,
    "operation": null,
    "upstream": null,
    "changes": [
      {
        "path": "main.rs",
        "from": null,
        "kind": "modified",
        "insertions": 1,
        "deletions": 1,
        "binary": false
      }
    ],
    "insertions": 1,
    "deletions": 1,
    "open": {
      "id": "033a2e4a5d79bfc05f855c6db5230b9c77a4fe6b",
      "change_id": "qprtzzrrxtsnzlllvrzstmowwwzmxvlx",
      "pending": "616d14a37ceb2c96501cb19185ff385c637fe4d3",
      "subject": null,
      "clean": false,
      "base": "e629c0d9955c9184bb66538b09463db90f77c9f2",
      "time": 1789331633
    },
    "parent": {
      "id": "e629c0d9955c9184bb66538b09463db90f77c9f2",
      "change_id": "nspyotlrrxvwmxponuqykpkwxysyvlnp",
      "subject": "parser: skeleton",
      "time": 1789331633,
      "segment": "b5c58c9c980cccdf25ddcd46ce94927485f5e0b6",
      "signed": false
    },
    "conflicts": [],
    "foreign": null,
    "futures": {
      "base": null,
      "remote": null,
      "remote_unnamed": false
    },
    "session": null,
    "held": null,
    "resolving": null,
    "last_op": {
      "id": "bf4bea5413f13a515a85591880eedc298264db6e",
      "short_id": "bf4b",
      "kind": "op",
      "verb": "commit",
      "summary": "commit on main: parser: skeleton",
      "time": 1789331633,
      "branch": "main",
      "session": null,
      "undo_of": null
    }
  }
}
```

Reading it:

- **`root`, `worktree`** — the checkout root, absolute and canonical, and which worktree this is: `id` is the name its operation chain is keyed by, `main` for the main worktree; `linked` says whether this is a linked worktree of another checkout, and `main` is that checkout's root.
- **`base`** — what the branch sits on, when it sits on anything: `name`, `ref`, `tip`, `role` (`trunk` or `parent`), and `above`, the commits reachable from the branch tip and not from the base. Null on trunk, detached, unborn, and inside an editing session — exactly when `futures.base` is null.
- **`remote`** — the remote the branch answers to, by its own `branch.<name>.remote` or the repository default; null when there is none or none can be named.
- **`changes`** — every uncommitted path with per-file counts. `kind` is `modified`, `added`, `deleted`, `renamed` or `copied` (those two carry the source path in `from`), `type_change`, or `intent_to_add`. `binary` marks files whose counts are not line counts.
- **`open`** — the open change. `clean` says whether the tree matches the commit beneath it, `change_id` is its identity — the letters column, null until a capture or a describe mints one — and `pending` is the pending description commit when one exists. `id` is the capture operation holding its current state, the op anchor, not the column.
- **`parent`** — the commit beneath the open change: its `change_id` is the letters column, and `segment` is the capture the commit was cut from, when one on this chain answers to it.
- **`futures`** — the pull verdicts the human header compresses into one line. Each side, when present, holds what it is measured `against` and a `verdict` such as `{"kind":"up-to-date","ahead":0}`.
- **`upstream`** — `ahead`, `behind`, and `gone`, when a remote tracking branch exists.
- **`foreign`, `held`, `resolving`** — null except when raw git drifted behind fufu's back, a rewrite is [held](../concepts/held-rewrites.md), or a resolve session is open. `resolving.session` names the session branch, and `resolving.here` says whether HEAD is on it; on the branch the hold stands on, `here` is false and `held` is set.
- **`last_op`** — the newest operation on this worktree's chain, as `ff op log --json` spells its row: `kind` (`op`, `capture`, `foreign`, or `note`), `verb`, `summary`, `branch`, and `session`. Status's own capture is never the row, so a dirty read still names the last thing that happened before it; foreign motion is reconciled first, so a `foreign` row agrees with `foreign`.

A script that checks `foreign`, `held`, and `resolving` before acting knows whether the repository needs a human first.

## `ff log --json`

The timeline as data: one object per commit, plus the open change as its own block rather than a fake row.

```console
$ ff log --json -n 1 | jq .
{
  "ff": 1,
  "cmd": "log",
  "data": {
    "commits": [
      {
        "id": "e629c0d9955c9184bb66538b09463db90f77c9f2",
        "short_id": "e629c0d9",
        "subject": "parser: skeleton",
        "author_name": "Ada Lovelace",
        "author_email": "ada@example.com",
        "time": 1789331633,
        "signed": false,
        "change_id": "nspyotlrrxvwmxponuqykpkwxysyvlnp",
        "session": null
      }
    ],
    "open": {
      "branch": "main",
      "id": "033a2e4a5d79bfc05f855c6db5230b9c77a4fe6b",
      "change_id": "qprtzzrrxtsnzlllvrzstmowwwzmxvlx",
      "base": "e629c0d9955c9184bb66538b09463db90f77c9f2",
      "subject": null,
      "time": 1789331633,
      "clean": false,
      "pending": "616d14a37ceb2c96501cb19185ff385c637fe4d3",
      "pending_short": "616d14a3"
    }
  }
}
```

A commit's `change_id` is the identity it keeps through rewrites, the letters column the human view prints; the open block's `change_id` is the id its commit will carry. A commit's `session` names the [session tag](#sessions-tagging-work-and-asking-about-it) it was made under, when there was one — which is how a supervisor tells an agent's commits from a person's in the same history. `ff show --json` carries `change_id` too.

## `ff evolog --json`

A change's history as data. Bare, or on `@`, the open change's captures, newest first, with the open change's id on the envelope:

```console
$ ff evolog --json -n 1 | jq .
{
  "ff": 1,
  "cmd": "evolog",
  "data": {
    "change_id": "qprtzzrrxtsnzlllvrzstmowwwzmxvlx",
    "snapshots": [
      {
        "id": "033a2e4a5d79bfc05f855c6db5230b9c77a4fe6b",
        "short_id": "033a",
        "subject": "pre: ff status --json",
        "time": 1789331633,
        "base": "e629c0d9955c9184bb66538b09463db90f77c9f2",
        "prev": "b5c58c9c980cccdf25ddcd46ce94927485f5e0b6",
        "session": null
      }
    ]
  }
}
```

On a revision, the operations that produced a commit carrying the change's id, on every worktree's chain, newest first, and the captures behind its close:

```console
$ ff evolog HEAD --json | jq .
{
  "ff": 1,
  "cmd": "evolog",
  "data": {
    "change_id": "nspyotlrrxvwmxponuqykpkwxysyvlnp",
    "commit": "e629c0d9955c9184bb66538b09463db90f77c9f2",
    "operations": [
      {
        "id": "bf4bea5413f13a515a85591880eedc298264db6e",
        "short_id": "bf4b",
        "chain": "main",
        "verb": "commit",
        "summary": "commit on main: parser: skeleton",
        "time": 1789331633,
        "commit": "e629c0d9955c9184bb66538b09463db90f77c9f2",
        "session": null
      }
    ],
    "snapshots": [
      {
        "id": "b5c58c9c980cccdf25ddcd46ce94927485f5e0b6",
        "short_id": "b5c5",
        "subject": "pre: ff commit -m parser: skeleton",
        "time": 1789331633,
        "base": null,
        "prev": null,
        "session": null
      }
    ]
  }
}
```

An operation's `commit` is the commit it produced for this change, which after a reword or restack differs from the one asked about; `chain` names the worktree whose chain it is on. `-p` adds `changes`, `insertions`, and `deletions` to each snapshot, as `ff diff --json` spells them. A commit without a `change-id` header has an empty `operations` and the captures on this chain that match it.

## `ff history --json`

The undo map as data: one object per keystroke of [`ff undo`](../reference/cli/undo.md), which is not the same thing as one object per operation. A run of adjacent captures collapses into the single step it undoes as, exactly as the human rendering draws it — [Snapshots and undo](../concepts/snapshots-and-undo.md) has the model.

```console
$ ff history --json | jq -c '.data.steps[]'
{"id":"002422158476205b28a05e88e880b6b107baa74c","short_id":"0024","landing":"now","kind":"capture","summary":"manual","time":1789331633,"branch":"main","session":"flight-3","collapsed":0,"distance":0}
{"id":"033a2e4a5d79bfc05f855c6db5230b9c77a4fe6b","short_id":"033a","landing":"undo","kind":"capture","summary":"pre: ff status --json","time":1789331633,"branch":"main","session":null,"collapsed":2,"distance":1}
{"id":"bf4bea5413f13a515a85591880eedc298264db6e","short_id":"bf4b","landing":"undo","kind":"op","summary":"commit on main: parser: skeleton","time":1789331633,"branch":"main","session":null,"collapsed":1,"distance":2}
{"id":"b5c58c9c980cccdf25ddcd46ce94927485f5e0b6","short_id":"b5c5","landing":"undo","kind":"capture","summary":"pre: ff commit -m parser: skeleton","time":1789331633,"branch":"main","session":null,"collapsed":1,"distance":3}
{"id":"8d3f32d135b4c8deea13646577f265332a1de6d2","short_id":"8d3f","landing":"undo","kind":"note","summary":"operation log initialized from observed state; earlier operations not undoable","time":1789331633,"branch":"main","session":null,"collapsed":1,"distance":4}
```

`landing` is `now` for where the repository stands, `undo` for each step below it, and `redo` for steps above after an undo. Redo rows carry negative `distance`, so `distance` alone says how many presses in which direction.

`collapsed` is how many operations the step folds together. `kind` sorts operations: `op` is a verb somebody ran, `capture` is an automatic snapshot, and `note` records something that moved no tree, such as a push or the log's floor.

The envelope also carries `data.floor`, true when the log bottoms out at the initialized-from-observed-state entry. Every `id` here is a valid argument to the [`ff op`](../reference/cli/op.md) verbs.

## Exit codes

Five codes, one meaning each:

| code | meaning |
| --- | --- |
| 0 | done — or yes, for a command that answers a question |
| 1 | no — the command failed, or the check's answer is negative |
| 2 | the command line was wrong |
| 3 | held or blocked, or a dry run predicts a hold; earlier successful updates can stand |
| 4 | contended — retry the command with a bounded retry policy; ambient work may already have run |

The code follows the error id: `usage/*` errors exit 2, `held/*` errors exit 3, `ref/contended` exits 4, everything else exits 1.

Exit 3 also rides a `data` envelope. [`ff pull`](../reference/cli/pull.md), [`ff restack`](../reference/cli/restack.md), [`ff done`](../reference/cli/done.md), [`ff lift`](../reference/cli/lift.md), and [`ff absorb`](../reference/cli/absorb.md) report held replays this way. [`ff push`](../reference/cli/push.md) reports a branch blocked by an existing hold. [`ff doctor`](../reference/cli/doctor.md) does the same at 1, its findings as `data` and the code as the verdict. A script reads the envelope for what happened and the code for whether to stop.

A reword's cascade is an exception: [`ff describe`](../reference/cli/describe.md) currently exits 0 even if `reword.cascade.held` is nonempty. Multi-branch push exits 1 for a refusal even if another branch was blocked by a hold. Exit codes do not imply that every branch succeeded or that no earlier updates landed; inspect the report.

The rewrite verbs' reports — `ff restack`, `ff pull`, `ff done`, `ff lift`, `ff absorb`, and each branch under `cascade.moved` — carry `dropped`: the commits the replay removed rather than rewrote. Each entry has `old`, the commit's full sha, `subject`, and `reason`, either `empty`, its replayed tree matched its new parent's, or `superseded`, the base already holds a commit with its change id; under `superseded`, `by` is that base commit's full sha.

- **Exit 3** is the code git has no use for, because only a tool that lands if clean produces the outcome. `ff pull` exiting 3 is a scriptable "the base moved and this needs you": the [held rewrite](../concepts/held-rewrites.md) — the replay recorded and waiting rather than applied — is parked on the branch that conflicted, whatever the run landed on other branches stands, and the script should stop and surface it rather than retry.
- **Exit 4** asks the opposite. Another writer held the ref for a moment, so retry the same command — with a cap, because a lock file nobody clears gives the same answer every time.
- **Exit 1** is also `ff doctor`'s verdict, 0 healthy and 1 findings, so CI can gate on it.

Strict mode is where exit 2 earns attention. With `fufu.gitPolicy` set to `strict`, [`ff git <word>`](../reference/cli/git.md) refuses mapped Git writes before capture or Git execution. It still records the policy tally:

```console
$ ff config gitPolicy strict
gitPolicy = strict (this repo)
$ ff git checkout -b topic
ff: fufu.gitPolicy is strict, and fufu has a verb for git checkout: ff switch — git's checkout was two jobs: ff switch moves, ff restore brings files back
  try:
    ff switch
    ff config gitPolicy coach
$ echo $?
2
```

The refusal is a usage error — id `usage/git-policy` — because the command line itself is what policy rejects. No capture or Git execution occurred; the exits name the fufu verb to type instead. `ff git rebase` is refused too, with `ff restack` as the suggestion. Passthrough words such as `ff git merge` and `ff git bisect` still run. Agent hooks have a different order: capture is attempted before policy evaluation. [Agent setup](setup.md) covers the tiers and limits.

One more contract keeps scripts out of stuck states: no verb ever blocks on a prompt or an editor with nobody there to answer. Wherever fufu would ask, a flag supplies the answer up front, and when stdin is not a terminal — or `FF_NONINTERACTIVE` is set to force it — the question becomes a structured error naming that flag, such as [`ff describe`](../reference/cli/describe.md) with no `-m` failing instead of opening an editor.

## Extensions

`ff <name>` runs `ff-<name>` from PATH when no built-in verb matches, which is git's own extension model. fufu captures the worktree, sets three variables, and runs it: `FF_REPO` is the worktree it was invoked against, unset outside one; `FF_CONTRACT` is the envelope version above; `FF_SESSION` is the session tag when one is set.

Nothing else passes, and fufu says nothing about the verb: `ff help <name>` does not reach it, and `ff doctor` names every `ff-<name>` it finds on PATH in one `info` row.

## Piped output never pages

The log family — [`ff log`](../reference/cli/log.md), [`ff evolog`](../reference/cli/evolog.md), `ff op log` — pages on a terminal, git-style: `fufu.pager`, then `FF_PAGER`, then `PAGER`, then `less`. A pager spawns only when stdout is a real TTY and the view is human.

Piped output and `--json` never page, so a script never needs `| cat`, never inherits a hung `less`, and never sees pager chrome in its bytes.

Color follows the same discipline. ANSI is emitted only where a terminal will read it, and `NO_COLOR` is honored, so piped human output is plain text.

## Detecting fufu programmatically

[`ff version --json`](../reference/cli/version.md) answers from anywhere, repository or not, and is the cheapest "is fufu here, and which one" probe — the fields are `version`, `commit`, `date`, and the update status, so a caller never takes the display string apart.

Inside a directory, any reader says whether a repository is present by its error id:

```console
$ ff status --json
{"ff":1,"cmd":"status","error":{"id":"repo/not-found","message":"not a git repository (or any parent): Could not find a git repository in '.' or in any of its parents","exits":["ff init","ff clone <url>"]}}
$ echo $?
1
```

In a git repository, the readers simply work — fufu arms itself on first contact, and the operation log's first entry is the floor operation `operation log initialized from observed state; earlier operations not undoable`. Whether the safety net is actually wired in — hooks installed, gc guard set, reflogs present — is [`ff doctor --json`](../reference/cli/doctor.md)'s question, and its exit code is the verdict.

An extension gets the answers handed to it. `ff <name>` runs `ff-<name>` from PATH when no built-in verb matches, and the child inherits `FF_REPO` (the worktree it was invoked against, unset outside one), `FF_CONTRACT` (the envelope version it is about to parse), and `FF_SESSION` (the session tag when one is set).

For the repository root in any other context, `ff git rev-parse --show-toplevel` is the passthrough spelling.

## Reading the operation log from a script

[`ff op log --json`](../reference/cli/op-log.md) is the record of everything fufu did, newest first — one object per operation, captures included:

```console
$ ff op log --json | jq -c '.data.ops[]'
{"id":"002422158476205b28a05e88e880b6b107baa74c","short_id":"0024","kind":"capture","verb":"","summary":"manual","time":1789331633,"branch":"main","session":"flight-3","undo_of":null}
{"id":"ae38c7d110818c4c40474bafd3f0d4c9ebe79a69","short_id":"ae38","kind":"capture","verb":"","summary":"manual","time":1789331633,"branch":"main","session":"flight-3","undo_of":null}
{"id":"033a2e4a5d79bfc05f855c6db5230b9c77a4fe6b","short_id":"033a","kind":"capture","verb":"","summary":"pre: ff status --json","time":1789331633,"branch":"main","session":null,"undo_of":null}
{"id":"bf4bea5413f13a515a85591880eedc298264db6e","short_id":"bf4b","kind":"op","verb":"commit","summary":"commit on main: parser: skeleton","time":1789331633,"branch":"main","session":null,"undo_of":null}
{"id":"b5c58c9c980cccdf25ddcd46ce94927485f5e0b6","short_id":"b5c5","kind":"capture","verb":"","summary":"pre: ff commit -m parser: skeleton","time":1789331633,"branch":"main","session":null,"undo_of":null}
{"id":"8d3f32d135b4c8deea13646577f265332a1de6d2","short_id":"8d3f","kind":"note","verb":"init","summary":"operation log initialized from observed state; earlier operations not undoable","time":1789331633,"branch":"main","session":null,"undo_of":null}
```

`verb` names which fufu verb an `op` was; a capture's `summary` says what it ran ahead of — the `pre:` prefix is literal, because operations are written before the mutation they describe, so an entry is a claim about the next moment rather than a report on the last one. `undo_of` links an operation to the one it reversed, when it was one.

An operation id addresses the operation everywhere the `ff op` family takes one, and the shortest unique prefix is enough — `short_id` is exactly that prefix. [`ff op show`](../reference/cli/op-show.md) reads one out whole, ref transitions included:

```console
$ ff op show bf4bea5413f13a515a85591880eedc298264db6e --json | jq .
{
  "ff": 1,
  "cmd": "op show",
  "data": {
    "id": "bf4bea5413f13a515a85591880eedc298264db6e",
    "kind": "op",
    "summary": "commit on main: parser: skeleton",
    "time": 1789331633,
    "branch": "main",
    "session": null,
    "base": null,
    "prev": "b5c58c9c980cccdf25ddcd46ce94927485f5e0b6",
    "tree": "5d90422423db5ef6b431e8b9e60e0baf04b8742a",
    "refs": [
      {
        "name": "refs/heads/main",
        "old": null,
        "new": "e629c0d9955c9184bb66538b09463db90f77c9f2"
      }
    ],
    "changes": [],
    "insertions": 0,
    "deletions": 0
  }
}
```

From there the rest of the family acts on the same ids. [`ff op restore <id>`](../reference/cli/op-restore.md) restores the current worktree's recorded local state, subject to worktree guards. [`ff op diff`](../reference/cli/op-diff.md) compares files in two operation trees, not ref transitions, and `--at-op <id>` reads a path as it stood at one.

Operations and revisions are separate address spaces: `--at-op` reads an operation, while `--from` and revision arguments read commits or change IDs. The wrong kind is refused as `usage/op-in-rev-position` or `usage/rev-in-op-position`. Change IDs use k–z, not the operation's hex alphabet. A unique change-ID prefix names its commit in a revision slot; a prefix of the open change's ID names `@`. A change on multiple visible commits is refused as `usage/revset-divergent`. Other operation-taking flags, such as `ff watch --since`, follow the operation address space too.

### Sessions: tagging work, and asking about it

A session is a tag on an operation, and nothing more. Set one — `--session <name>` on any invocation, or `FF_SESSION` in the environment, which is how the agent hooks stamp their own ids, or failing both the session the client is running (`CLAUDE_CODE_SESSION_ID` under Claude Code) — and every operation recorded while it is set carries it. There is no open, no close, and nothing to clean up after a crash. Asking is a filter in the [op log's](../reference/cli/op-log.md) set language:

```console
$ ff op log 'session(flight-3)' --json | jq -c '.data.ops[]'
{"id":"002422158476205b28a05e88e880b6b107baa74c","short_id":"0024","kind":"capture","verb":"","summary":"manual","time":1789331633,"branch":"main","session":"flight-3","undo_of":null}
{"id":"ae38c7d110818c4c40474bafd3f0d4c9ebe79a69","short_id":"ae38","kind":"capture","verb":"","summary":"manual","time":1789331633,"branch":"main","session":"flight-3","undo_of":null}
```

`kind(capture)`, `kind(op)`, and the rest of the grammar compose the same way, so "everything agent flight-3 did that was a real verb" is one expression. Two agents interleaving in one repository stay separable forever, because the tag rides each operation rather than a range between two points.

For a consumer that wants the log pushed rather than polled, [`ff watch`](../reference/cli/watch.md) streams it: one JSON object per line as operations land, opening on a `start` line naming the tip. `--session` and `--kind` filter it, and `--all` merges every worktree into one stream.

It is a foreground process you started, not a daemon. When a trim rewrites the log under it, the stream says so and exits 1 — every id you were holding stopped resolving, so reconnect rather than carry on.
