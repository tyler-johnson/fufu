# The machine surface

**A script, a CI job, or an agent calling `ff` is a first-class reader: every reader takes `--json`, every failure carries a stable id, and every exit code means one thing.**

A verb computes one data model, and the human rendering and the JSON rendering are both consumers of it, never translations of each other. `--json` is therefore not the human layout re-serialized: `ff status` crops to what an eye wants, while its JSON carries the model whole — the full change list, the [open change](../concepts/changes.md), the parent commit, the sync futures. This is what keeps the two from drifting apart, and it is why a script should parse the JSON and never the display text.

Every transcript below is real `ff` output. Where one is piped through `jq .`, that is for the page's eye — the actual emission is always a single line.

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

`error.id` is the stable machine name: prose gets reworded, ids do not, so a script branches on the id and never matches a sentence. `error.exits` is the same block the human rendering prints — the commands somebody would type next — handed to the machine as data. [`ff explain <id>`](../reference/cli/explain.md) turns any id back into prose on demand. [The error id index](../reference/errors.md) lists every id with its exit code, and `ff explain --list` prints the same table from the binary.

What is promised: within one contract version, a field keeps its name and its meaning, and the envelope keeps its shape. New fields may appear as the surface grows, so take fields by name and tolerate ones you do not know. A change that breaks an existing field is what bumps the `ff` number, which is why a strict consumer asserts it before parsing. The human rendering promises none of this — layout, wording, and color are free to change in any release.

Timestamps are unix seconds, always named `time`. Commit ids are hex; operation ids are spelled in the letters k–z, never hex — see [Snapshots and undo](../concepts/snapshots-and-undo.md) for why the two address spaces never mix.

## `ff status --json`

The whole working-tree model in one read: where you are, what changed, the open change, its parent, conflicts, foreign drift, and what a sync would do.

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
      "commit": "64962838a9353e3a4c3e78677f1bc6348b328058"
    },
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
      "id": "7143039278c53a615299fd80e3556b67440f73fb",
      "id_letters": "syvwzwqxsrnuwptyuxqqkmrzlwuutotsvvzkswko",
      "pending": "f3434ddb0952f80978cb5a2ea8ad6b2180d41ef1",
      "subject": null,
      "clean": false,
      "base": "64962838a9353e3a4c3e78677f1bc6348b328058",
      "time": 1787985378
    },
    "parent": {
      "id": "64962838a9353e3a4c3e78677f1bc6348b328058",
      "subject": "parser: skeleton",
      "time": 1787985378,
      "segment": "cb1d2bc315064c26037a3ef212e12f7243633bee"
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
    "resolving": null
  }
}
```

Reading it: `changes` is every uncommitted path with per-file counts — `kind` is `modified`, `added`, `deleted`, `renamed` or `copied` (those two with `from` carrying the source path), `type_change`, or `intent_to_add`, and `binary` marks files whose counts are not line counts. `open` is the open change: `clean` says whether the tree matches the commit beneath it, `id_letters` is the operation id of the capture holding its current state, and `pending` is the pending description commit when one exists. `futures` carries the sync verdicts the human header compresses into one line — each side, when present, holds what it is measured `against` and a `verdict` such as `{"kind":"up-to-date","ahead":0}`. `upstream`, when a remote tracking branch exists, carries `ahead`, `behind`, and `gone`. `foreign`, `held`, and `resolving` are null except when raw git drifted behind fufu's back, a rewrite is [held](../concepts/held-rewrites.md), or a resolve session is open — a script that checks those three fields before acting knows whether the repository needs a human first.

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
        "session": null
      }
    ],
    "open": {
      "branch": "main",
      "id": "38db22cc13e4fcd1cf8c28771a1d4014861cc7dc",
      "id_letters": "wrmoxxnnywlvknmynkrnxrssypymvzyvrtynnsmn",
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

A commit's `session` names the [session tag](#sessions-tagging-work-and-asking-about-it) it was made under, when there was one — which is how a supervisor tells an agent's commits from a person's in the same history.

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

`landing` is `now` for where the repository stands, `undo` for each step below it, and `redo` for steps above after an undo — redo rows carry negative `distance`, so `distance` alone says how many presses in which direction. `collapsed` is how many operations the step folds together. `kind` sorts operations: `op` is a verb somebody ran, `capture` is an automatic snapshot, `note` records something that moved no tree — a publish, the log's floor. The envelope also carries `data.floor`, true when the log bottoms out at the initialized-from-observed-state entry. Every `id` here is a valid argument to the `ff op` verbs.

## Exit codes

Five codes, one meaning each:

| code | meaning |
| --- | --- |
| 0 | done — or yes, for a command that answers a question |
| 1 | no — the command failed, or the check's answer is negative |
| 2 | the command line was wrong |
| 3 | held — nothing was touched, and a human decision is required |
| 4 | contended — nothing was touched, and the same command run again is the answer |

The code follows the error id: `usage/*` errors exit 2, `held/*` errors exit 3, `ref/contended` exits 4, everything else exits 1. Exit 3 is the code git has no use for, because only a tool with land-if-clean produces the outcome: [`ff sync`](../reference/cli/sync.md) exiting 3 is a scriptable "the base moved and this needs you" — nothing moved, the [held rewrite](../concepts/held-rewrites.md) is parked, and the script should stop and surface it rather than retry. Exit 4 asks the opposite: another writer held the ref for a moment, so retry the same command, with a cap, because a lock file nobody clears gives the same answer every time. [`ff doctor`](../reference/cli/doctor.md) uses 1 as its verdict — 0 healthy, 1 findings — so CI can gate on it.

Strict mode is where exit 2 earns attention. With `fufu.gitPolicy` set to `strict`, `ff git <word>` refuses any git word fufu has a verb for, before the capture and before anything runs:

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

One more contract keeps scripts out of stuck states: no verb ever blocks on a prompt or an editor with nobody there to answer. Wherever fufu would ask, a flag supplies the answer up front, and when stdin is not a terminal — or `FF_NONINTERACTIVE` is set to force it — the question becomes a structured error naming that flag, such as `ff describe` with no `-m` failing instead of opening an editor.

## The MCP surface

[`ff mcp`](../reference/cli/mcp.md) is the same contract over the Model Context Protocol: one tool, `ff`, whose input is the command line after `ff` as an array of words, and whose output is the envelope above. Every call runs the binary as a child with `--json` and relays what it printed, so nothing on this page changes for a caller that arrives through the tool — it is a shell over one contract, not a second implementation.

```json
{"name": "ff", "arguments": {"args": ["show", "doesnotexist"]}}
```

```json
{"content": [{"type": "text", "text": "{\"ff\":1,\"cmd\":\"show\",\"error\":{\"id\":\"usage/revset-unknown-revision\",\"message\":\"no revision here answers to `doesnotexist`\",\"exits\":[\"ff log\",\"ff branch\"]}}"}],
 "structuredContent": {"ff": 1, "cmd": "show", "error": {"id": "usage/revset-unknown-revision", "message": "no revision here answers to `doesnotexist`", "exits": ["ff log", "ff branch"]}},
 "isError": true}
```

The envelope arrives twice, as the text content and as `structuredContent`, so a client that reads either gets the whole of it. `isError` follows the exit code: false on 0, true on anything else, and a fufu failure is a *successful* tool call carrying it, never a protocol error, because a client renders a protocol error opaquely and the `error.id` inside is what the agent has to read. The exit-code rules restate as tool rules: an id under `held/*` means nothing moved and a person is needed, so the agent stops and says so; `ref/contended` means the same call run once more. A `help` call returns the page as text with no structured content, and the six verbs the tool does not offer — `git`, `update`, `watch`, `hook`, `unhook`, `mcp` — answer with `usage/mcp-verb-unavailable`. An optional `cwd` runs the call in another directory, and `--session` on the server tags every child's operations. [Agent setup](setup.md#serve-the-verbs-as-a-tool) covers registering it.

While the server is up for a Claude Code session, `fufu.toolPolicy` (default `strict`) refuses an `ff` the agent runs through its shell tool instead, on the hook's `PreToolUse` channel. The refusal is the same JSON shape as `gitPolicy`'s, and its reason names the tool and carries the `args` to call it with, so the agent can rewrite the call without a lookup:

```json
{"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny",
 "permissionDecisionReason": "fufu.toolPolicy is strict here and the ff tool is up: call the ff tool (mcp__plugin_fufu_fufu__ff) with {\"args\":[\"status\"]} instead of running ff in the shell — load the tool's schema first if it is deferred"}}
```

`--json` is dropped from the args, since the tool adds it. The six shell-only verbs are never refused, and nothing is said at all when no server is serving that client.

## Piped output never pages

The log family — `ff log`, `ff evolog`, `ff op log` — pages on a terminal, git-style: `fufu.pager`, then `FF_PAGER`, then `PAGER`, then `less`. A pager spawns only when stdout is a real TTY and the view is human. Piped output and `--json` never page, so a script never needs `| cat`, never inherits a hung `less`, and never sees pager chrome in its bytes. Color follows the same discipline: ANSI is emitted only where a terminal will read it, and `NO_COLOR` is honored, so piped human output is plain text.

## Detecting fufu programmatically

`ff version --json` answers from anywhere, repository or not, and is the cheapest "is fufu here, and which one" probe — the fields are `version`, `commit`, `date`, and the update status, so a caller never takes the display string apart.

Inside a directory, any reader says whether a repository is present by its error id:

```console
$ ff status --json
{"ff":1,"cmd":"status","error":{"id":"repo/not-found","message":"not a git repository (or any parent): Could not find a git repository in '.' or in any of its parents","exits":["ff init","ff clone <url>"]}}
$ echo $?
1
```

In a git repository, the readers simply work — fufu arms itself on first contact, and the operation log's first entry is the floor operation `operation log initialized from observed state; earlier operations not undoable`. Whether the safety net is actually wired in — hooks installed, gc guard set, reflogs present — is [`ff doctor --json`](../reference/cli/doctor.md)'s question, and its exit code is the verdict.

An extension gets the answers handed to it: `ff <name>` runs `ff-<name>` from PATH when no built-in verb matches, and the child inherits `FF_REPO` (the worktree it was invoked against, unset outside one), `FF_CONTRACT` (the envelope version it is about to parse), and `FF_SESSION` (the session tag when one is set). For the repository root in any other context, `ff git rev-parse --show-toplevel` is the passthrough spelling.

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

From there the rest of the family acts on the same ids: [`ff op restore <id>`](../reference/cli/op-restore.md) rewinds the whole repository to one, [`ff op diff`](../reference/cli/op-diff.md) compares two, and `--at-op <id>` on the verbs that take it reads a path as it stood at one — which is also the only place an operation id is legal outside the `ff op` family, because operations and revisions are separate address spaces and passing one where the other belongs is a refused error, not a convenience.

### Sessions: tagging work, and asking about it

A session is a tag on an operation, and nothing more. Set one — `--session <name>` on any invocation, or `FF_SESSION` in the environment, which is how the agent hooks stamp their own ids — and every operation recorded while it is set carries it. There is no open, no close, and nothing to clean up after a crash. Asking is a filter in the [op log's](../reference/cli/op-log.md) set language:

```console
$ ff op log 'session(flight-3)' --json | jq -c '.data.ops[]'
{"id":"wrmoxxnnywlvknmynkrnxrssypymvzyvrtynnsmn","short_id":"wrmo","kind":"capture","verb":"","summary":"manual","time":1787985391,"branch":"main","session":"flight-3","undo_of":null}
{"id":"pwknzqxqpurrvwokmqnyvuovmzumukxlpmmztywr","short_id":"pwkn","kind":"capture","verb":"","summary":"manual","time":1787985391,"branch":"main","session":"flight-3","undo_of":null}
```

`kind(capture)`, `kind(op)`, and the rest of the grammar compose the same way, so "everything agent flight-3 did that was a real verb" is one expression. Two agents interleaving in one repository stay separable forever, because the tag rides each operation rather than a range between two points.

For a consumer that wants the log pushed rather than polled, [`ff watch`](../reference/cli/watch.md) streams it: one JSON object per line as operations land, opening on a `start` line naming the tip, with `--session` and `--kind` filters and `--all` to merge every worktree into one stream. It is a foreground process you started, not a daemon, and when a trim rewrites the log under it the stream says so and exits 1 — every id you were holding stopped resolving, so reconnect rather than carry on.
