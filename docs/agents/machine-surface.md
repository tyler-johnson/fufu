# The machine surface

**A script, a CI job, or an agent calling `ff` is a first-class reader: every reader takes `--json`, every failure carries a stable id, and every exit code means one thing.**

A verb computes one data model, and the human rendering and the JSON rendering are both readers of it. Neither is a translation of the other.

So `--json` is not the human layout re-serialized. [`ff status`](../reference/cli/status.md) crops to what an eye wants, while its JSON carries the model whole: the full change list, the [open change](../concepts/changes.md), the parent commit, the pull futures.

That is what keeps the two from drifting apart, and it is why a script should parse the JSON and never the display text.

> Every transcript below is real `ff` output. Where one is piped through `jq .`, that is for the page's eye — the actual emission is always a single line.

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

`error.id` is the stable machine name. Prose gets reworded and ids do not, so a script branches on the id and never matches a sentence. `error.exits` is the same block the human rendering prints — the commands somebody would type next — handed over as data.

[`ff explain <id>`](../reference/cli/explain.md) turns any id back into prose on demand. [The error id index](../reference/errors.md) lists every id with its exit code, and `ff explain --list` prints the same table from the binary.

### What is promised

Within one contract version, a field keeps its name and its meaning, and the envelope keeps its shape. New fields may appear as the surface grows, so take fields by name and tolerate ones you do not know.

A change that breaks an existing field is what bumps the `ff` number, which is why a strict consumer asserts it before parsing.

The human rendering promises none of this. Layout, wording, and color are free to change in any release.

Timestamps are unix seconds, always named `time`. Commit ids and operation ids are hex, forty characters in JSON, and the slot decides which a hex prefix means — a revision slot reads a sha or a change id, an operation slot an operation id. A `change_id` is a commit's identity across rewrites, spelled in the letters k–z and never hex: the letters column the human views print. See [Snapshots and undo](../concepts/snapshots-and-undo.md).

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
      "commit": "cd5c0d35d19736cef307912620fcd77e0dce1645"
    },
    "root": "/home/tyler/parser",
    "worktree": {
      "id": "main",
      "linked": false,
      "main": "/home/tyler/parser"
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
      "id": "04c78631534881f3319290940ee156e5b89dfe9a",
      "change_id": "nyrszqtkznwuykyxoskttupyppplxnwu",
      "pending": "b052c0d32a8637271add239049f1e06e87ffb55e",
      "subject": null,
      "clean": false,
      "base": "cd5c0d35d19736cef307912620fcd77e0dce1645",
      "time": 1788748661
    },
    "parent": {
      "id": "cd5c0d35d19736cef307912620fcd77e0dce1645",
      "change_id": "lqsypptxyrxlkonpnolqqtovsvzmuzwt",
      "subject": "parser: skeleton",
      "time": 1788748661,
      "segment": "a34ed99aa8b4c4fee5ad18eedd811fd098767d54",
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
      "id": "utoozzykkzxtqwmntvoptlxxuqxszpvlwpyrmkol",
      "short_id": "utoo",
      "kind": "op",
      "verb": "commit",
      "summary": "commit on main: parser: skeleton",
      "time": 1788748661,
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
        "id": "64962838a9353e3a4c3e78677f1bc6348b328058",
        "short_id": "64962838",
        "subject": "parser: skeleton",
        "author_name": "Tyler Johnson",
        "author_email": "tyler@tylerjohnson.me",
        "time": 1787985378,
        "signed": false,
        "change_id": "wrmoxxnnkqvtpsrkywlvzxynnmoslryu",
        "session": null
      }
    ],
    "open": {
      "branch": "main",
      "id": "38db22cc13e4fcd1cf8c28771a1d4014861cc7dc",
      "change_id": "qtwplrskwswwkymmtlynvxrlzvwvurzs",
      "base": "64962838a9353e3a4c3e78677f1bc6348b328058",
      "subject": null,
      "time": 1787985391,
      "clean": false,
      "pending": "bdbf5c8c8157f25cbd4dfc422840924899109b47",
      "pending_short": "bdbf5c8c"
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
    "change_id": "qtwplrskwswwkymmtlynvxrlzvwvurzs",
    "snapshots": [
      {
        "id": "38db22cc13e4fcd1cf8c28771a1d4014861cc7dc",
        "short_id": "38db",
        "subject": "pre: ff status",
        "time": 1787985391,
        "base": "64962838a9353e3a4c3e78677f1bc6348b328058",
        "prev": null,
        "session": null
      }
    ]
  }
}
```

On a revision, the operations that produced a commit carrying the change's id, on every worktree's chain, newest first, and the captures behind its close:

```console
$ ff evolog --json wrmoxxnn | jq .
{
  "ff": 1,
  "cmd": "evolog",
  "data": {
    "change_id": "wrmoxxnnkqvtpsrkywlvzxynnmoslryu",
    "commit": "64962838a9353e3a4c3e78677f1bc6348b328058",
    "operations": [
      {
        "id": "ksrnsmvwzxopnqxxrslukyzuvquqkrlvqmkrxrqr",
        "short_id": "ksrn",
        "chain": "main",
        "verb": "commit",
        "summary": "commit on main: parser: skeleton",
        "time": 1787985378,
        "commit": "64962838a9353e3a4c3e78677f1bc6348b328058",
        "session": null
      }
    ],
    "snapshots": [ ... ]
  }
}
```

An operation's `commit` is the commit it produced for this change, which after a reword or restack differs from the one asked about; `chain` names the worktree whose chain it is on. `-p` adds `changes`, `insertions`, and `deletions` to each snapshot, as `ff diff --json` spells them. A commit without a `change-id` header has an empty `operations` and the captures on this chain that match it.

## `ff history --json`

The undo map as data: one object per keystroke of [`ff undo`](../reference/cli/undo.md), which is not the same thing as one object per operation. A run of adjacent captures collapses into the single step it undoes as, exactly as the human rendering draws it — [Snapshots and undo](../concepts/snapshots-and-undo.md) has the model.

```console
$ ff history --json | jq -c '.data.steps[]'
{"id":"wrmoxxnnywlvknmynkrnxrssypymvzyvrtynnsmn","short_id":"wrmo","landing":"now","kind":"capture","summary":"manual","time":1787985391,"branch":"main","session":"flight-3","collapsed":0,"distance":0}
{"id":"syvwzwqxsrnuwptyuxqqkmrzlwuutotsvvzkswko","short_id":"syvw","landing":"undo","kind":"capture","summary":"pre: ff status --json","time":1787985378,"branch":"main","session":null,"collapsed":2,"distance":1}
{"id":"ksrnsmvwzxopnqxxrslukyzuvquqkrlvqmkrxrqr","short_id":"ksrn","landing":"undo","kind":"op","summary":"commit on main: parser: skeleton","time":1787985378,"branch":"main","session":null,"collapsed":1,"distance":2}
{"id":"noymxonwyuztvnxtzwspwlkxyxlyxksxvwtwwoll","short_id":"noym","landing":"undo","kind":"capture","summary":"pre: ff commit -m parser: skeleton","time":1787985378,"branch":"main","session":null,"collapsed":1,"distance":3}
{"id":"qvtsvptlqsqszpyzykuwxkynmkoqnmpourkuwtol","short_id":"qvts","landing":"undo","kind":"note","summary":"operation log initialized from observed state; earlier operations not undoable","time":1787985378,"branch":"main","session":null,"collapsed":1,"distance":4}
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
| 3 | held — a human decision is required, and the branch that held was not touched |
| 4 | contended — nothing was touched, and the same command run again is the answer |

The code follows the error id: `usage/*` errors exit 2, `held/*` errors exit 3, `ref/contended` exits 4, everything else exits 1.

Exit 3 also rides a `data` envelope. When [`ff pull`](../reference/cli/pull.md), [`ff restack`](../reference/cli/restack.md), [`ff done`](../reference/cli/done.md), [`ff lift`](../reference/cli/lift.md), [`ff absorb`](../reference/cli/absorb.md), or [`ff push`](../reference/cli/push.md) holds a rewrite, the verb prints its full report as `data` and exits 3, because a held rewrite is an outcome with a report and not an error with an id. [`ff doctor`](../reference/cli/doctor.md) does the same at 1, its findings as `data` and the code as the verdict. A script reads the envelope for what happened and the code for whether to stop.

The rewrite verbs' reports — `ff restack`, `ff pull`, `ff done`, `ff lift`, `ff absorb`, and each branch under `cascade.moved` — carry `dropped`: the commits the replay removed rather than rewrote. Each entry has `old`, the commit's full sha, `subject`, and `reason`, either `empty`, its replayed tree matched its new parent's, or `superseded`, the base already holds a commit with its change id; under `superseded`, `by` is that base commit's full sha.

- **Exit 3** is the code git has no use for, because only a tool that lands if clean produces the outcome. `ff pull` exiting 3 is a scriptable "the base moved and this needs you": the [held rewrite](../concepts/held-rewrites.md) — the replay recorded and waiting rather than applied — is parked on the branch that conflicted, whatever the run landed on other branches stands, and the script should stop and surface it rather than retry.
- **Exit 4** asks the opposite. Another writer held the ref for a moment, so retry the same command — with a cap, because a lock file nobody clears gives the same answer every time.
- **Exit 1** is also `ff doctor`'s verdict, 0 healthy and 1 findings, so CI can gate on it.

Strict mode is where exit 2 earns attention. With `fufu.gitPolicy` set to `strict`, [`ff git <word>`](../reference/cli/git.md) refuses any git word fufu has a verb for, before the capture and before anything runs:

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

The refusal is a usage error — id `usage/git-policy` — because the command line itself is what policy rejects. Nothing was captured and nothing ran; the exits name the fufu verb to type instead. Git words fufu has no verb of its own for pass straight through under every policy, so a strict repository still runs `ff git rebase` or `ff git bisect` untouched. [Agent setup](setup.md) covers choosing a policy tier.

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
{"id":"wrmoxxnnywlvknmynkrnxrssypymvzyvrtynnsmn","short_id":"wrmo","kind":"capture","verb":"","summary":"manual","time":1787985391,"branch":"main","session":"flight-3","undo_of":null}
{"id":"pwknzqxqpurrvwokmqnyvuovmzumukxlpmmztywr","short_id":"pwkn","kind":"capture","verb":"","summary":"manual","time":1787985391,"branch":"main","session":"flight-3","undo_of":null}
{"id":"syvwzwqxsrnuwptyuxqqkmrzlwuutotsvvzkswko","short_id":"syvw","kind":"capture","verb":"","summary":"pre: ff status --json","time":1787985378,"branch":"main","session":null,"undo_of":null}
{"id":"ksrnsmvwzxopnqxxrslukyzuvquqkrlvqmkrxrqr","short_id":"ksrn","kind":"op","verb":"commit","summary":"commit on main: parser: skeleton","time":1787985378,"branch":"main","session":null,"undo_of":null}
{"id":"noymxonwyuztvnxtzwspwlkxyxlyxksxvwtwwoll","short_id":"noym","kind":"capture","verb":"","summary":"pre: ff commit -m parser: skeleton","time":1787985378,"branch":"main","session":null,"undo_of":null}
{"id":"qvtsvptlqsqszpyzykuwxkynmkoqnmpourkuwtol","short_id":"qvts","kind":"note","verb":"init","summary":"operation log initialized from observed state; earlier operations not undoable","time":1787985378,"branch":"main","session":null,"undo_of":null}
```

`verb` names which fufu verb an `op` was; a capture's `summary` says what it ran ahead of — the `pre:` prefix is literal, because operations are written before the mutation they describe, so an entry is a claim about the next moment rather than a report on the last one. `undo_of` links an operation to the one it reversed, when it was one.

An operation id addresses the operation everywhere the `ff op` family takes one, and the shortest unique prefix is enough — `short_id` is exactly that prefix. [`ff op show`](../reference/cli/op-show.md) reads one out whole, ref transitions included:

```console
$ ff op show ksrn --json | jq .
{
  "ff": 1,
  "cmd": "op show",
  "data": {
    "id": "ksrnsmvwzxopnqxxrslukyzuvquqkrlvqmkrxrqr",
    "kind": "op",
    "summary": "commit on main: parser: skeleton",
    "time": 1787985378,
    "branch": "main",
    "session": null,
    "base": null,
    "prev": "noymxonwyuztvnxtzwspwlkxyxlyxksxvwtwwoll",
    "tree": "5d90422423db5ef6b431e8b9e60e0baf04b8742a",
    "refs": [
      {
        "name": "refs/heads/main",
        "old": null,
        "new": "64962838a9353e3a4c3e78677f1bc6348b328058"
      }
    ],
    "changes": [],
    "insertions": 0,
    "deletions": 0
  }
}
```

From there the rest of the family acts on the same ids. [`ff op restore <id>`](../reference/cli/op-restore.md) rewinds the whole repository to one, [`ff op diff`](../reference/cli/op-diff.md) compares two, and `--at-op <id>` on the verbs that take it reads a path as it stood at one.

`--at-op` is also the only place an operation id is legal outside the `ff op` family. Operations and revisions are separate address spaces, and passing one where the other belongs is a refused error rather than a convenience: `usage/op-in-rev-position` one way, `usage/rev-in-op-position` the other. A change id, which shares the alphabet, is a revision: any prefix of one that is unique in the repository names its commit in a revision slot, a prefix of the open change's id is `@`, and a change standing on more than one visible commit is refused as `usage/revset-divergent`, naming each.

### Sessions: tagging work, and asking about it

A session is a tag on an operation, and nothing more. Set one — `--session <name>` on any invocation, or `FF_SESSION` in the environment, which is how the agent hooks stamp their own ids, or failing both the session the client is running (`CLAUDE_CODE_SESSION_ID` under Claude Code) — and every operation recorded while it is set carries it. There is no open, no close, and nothing to clean up after a crash. Asking is a filter in the [op log's](../reference/cli/op-log.md) set language:

```console
$ ff op log 'session(flight-3)' --json | jq -c '.data.ops[]'
{"id":"wrmoxxnnywlvknmynkrnxrssypymvzyvrtynnsmn","short_id":"wrmo","kind":"capture","verb":"","summary":"manual","time":1787985391,"branch":"main","session":"flight-3","undo_of":null}
{"id":"pwknzqxqpurrvwokmqnyvuovmzumukxlpmmztywr","short_id":"pwkn","kind":"capture","verb":"","summary":"manual","time":1787985391,"branch":"main","session":"flight-3","undo_of":null}
```

`kind(capture)`, `kind(op)`, and the rest of the grammar compose the same way, so "everything agent flight-3 did that was a real verb" is one expression. Two agents interleaving in one repository stay separable forever, because the tag rides each operation rather than a range between two points.

For a consumer that wants the log pushed rather than polled, [`ff watch`](../reference/cli/watch.md) streams it: one JSON object per line as operations land, opening on a `start` line naming the tip. `--session` and `--kind` filter it, and `--all` merges every worktree into one stream.

It is a foreground process you started, not a daemon. When a trim rewrites the log under it, the stream says so and exits 1 — every id you were holding stopped resolving, so reconnect rather than carry on.
