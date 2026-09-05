# The machine surface

**A script, a CI job, or an agent calling `ff` is a first-class reader: every reader takes `--json`, every failure carries a stable id, and every exit code means one thing.**

A verb computes one data model, and the human rendering and the JSON rendering are both readers of it. Neither is a translation of the other.

So `--json` is not the human layout re-serialized. [`ff status`](../reference/cli/status.md) crops to what an eye wants, while its JSON carries the model whole: the full change list, the [open change](../concepts/changes.md), the parent commit, the sync futures.

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

Timestamps are unix seconds, always named `time`. Commit ids are hex; operation ids are spelled in the letters k–z, never hex — see [Snapshots and undo](../concepts/snapshots-and-undo.md) for why the two address spaces never mix.

## `ff status --json`

The whole working-copy model in one read: where you are, what changed, the open change, its parent, conflicts, foreign drift, and what a sync would do.

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

Reading it:

- **`changes`** — every uncommitted path with per-file counts. `kind` is `modified`, `added`, `deleted`, `renamed` or `copied` (those two carry the source path in `from`), `type_change`, or `intent_to_add`. `binary` marks files whose counts are not line counts.
- **`open`** — the open change. `clean` says whether the tree matches the commit beneath it, `id_letters` is the operation id of the capture holding its current state, and `pending` is the pending description commit when one exists.
- **`futures`** — the sync verdicts the human header compresses into one line. Each side, when present, holds what it is measured `against` and a `verdict` such as `{"kind":"up-to-date","ahead":0}`.
- **`upstream`** — `ahead`, `behind`, and `gone`, when a remote tracking branch exists.
- **`foreign`, `held`, `resolving`** — null except when raw git drifted behind fufu's back, a rewrite is [held](../concepts/held-rewrites.md), or a resolve session is open.

A script that checks those last three fields before acting knows whether the repository needs a human first.

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

`landing` is `now` for where the repository stands, `undo` for each step below it, and `redo` for steps above after an undo. Redo rows carry negative `distance`, so `distance` alone says how many presses in which direction.

`collapsed` is how many operations the step folds together. `kind` sorts operations: `op` is a verb somebody ran, `capture` is an automatic snapshot, and `note` records something that moved no tree, such as a publish or the log's floor.

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

- **Exit 3** is the code git has no use for, because only a tool that lands if clean produces the outcome. [`ff sync`](../reference/cli/sync.md) exiting 3 is a scriptable "the base moved and this needs you": the [held rewrite](../concepts/held-rewrites.md) — the replay recorded and waiting rather than applied — is parked on the branch that conflicted, whatever the run landed on other branches stands, and the script should stop and surface it rather than retry.
- **Exit 4** asks the opposite. Another writer held the ref for a moment, so retry the same command — with a cap, because a lock file nobody clears gives the same answer every time.
- **Exit 1** is also [`ff doctor`](../reference/cli/doctor.md)'s verdict, 0 healthy and 1 findings, so CI can gate on it.

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

The envelope arrives twice, as the text content and as `structuredContent`, so a client that reads either gets the whole of it.

`isError` follows the exit code: false on 0, true on anything else. A fufu failure is a *successful* tool call carrying it, never a protocol error — a client renders a protocol error opaquely, and the `error.id` inside is what the agent has to read.

The exit-code rules restate as tool rules. An id under `held/*` means nothing moved and a person is needed, so the agent stops and says so. `ref/contended` means the same call run once more.

### What the tool serves

A `help` call returns the page as text with no structured content. The verbs the tool does not offer — `git`, `update`, `watch`, `hook`, `unhook`, `mcp`, and `extension` — answer with `usage/mcp-verb-unavailable`.

A declared extension is relayed the way a verb is. An undeclared one answers with `usage/mcp-extension-undeclared` and an exit naming `ff extension add <name>`. A declared one whose manifest says `undoable: false` answers with `usage/mcp-extension-not-undoable`, which costs it the args array and not the tools it produces.

An optional `cwd` runs the call in another directory, and `--session` on the server tags every child's operations.

Beside the one tool, the server lists a tool per descriptor a declared extension produced, named `<extension>__<tool>` and typed under [the tool list](../reference/extensions.md#optional-mcp-tools). The args array stays the route for every verb, an extension's included. [Agent setup](setup.md#serve-the-verbs-as-a-tool) covers registering it.

### `toolPolicy` in the shell

While the server is up for a Claude Code session, `fufu.toolPolicy` (default `strict`) refuses an `ff` the agent runs through its shell tool instead, on the hook's `PreToolUse` channel. The refusal is the same JSON shape as `gitPolicy`'s, and its reason names the tool and carries the `args` to call it with, so the agent can rewrite the call without a lookup:

```json
{"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny",
 "permissionDecisionReason": "fufu.toolPolicy is strict here and the ff tool is up: call the ff tool (mcp__plugin_fufu_fufu__ff) with {\"args\":[\"status\"]} instead of running ff in the shell — load the tool's schema first if it is deferred"}}
```

`--json` is dropped from the args, since the tool adds it.

What the refusal speaks to is the args array, the one route it can name. A builtin verb and a declared extension are both refused and pointed at the tool. The seven shell-only verbs pass, and so does any `ff <name>` the args array will not carry: one nobody declared, and one declaring `undoable: false`.

A non-undoable extension passes even when it produced tools of its own. The registry records that it promised tools and never which verb each one covers, so a refusal naming a `<name>__<verb>` that is not there would leave the verb nowhere to run.

A declared extension's refusal names it, so an agent can tell it from the undeclared one beside it. Nothing is said at all when no server is serving that client.

## Extensions

`ff <name>` runs `ff-<name>` from PATH when no built-in verb matches, which is git's own extension model. There are two kinds of extension, and what separates them is not what one is allowed to do — it is what fufu will say about it.

### Undeclared

An **undeclared** extension is any `ff-<name>` a PATH walk finds. fufu captures the worktree, sets three variables, and runs it: `FF_REPO` is the worktree it was invoked against, unset outside one; `FF_CONTRACT` is the envelope version above; `FF_SESSION` is the session tag when one is set.

Nothing else passes, and fufu says nothing about the verb. The tool refuses it with `usage/mcp-extension-undeclared` and an exit naming `ff extension add <name>`, it is not on the tool's card — the tool description an agent reads — and `ff help <name>` does not reach it.

Under `fufu.toolPolicy=strict` the shell refusal lets `ff <name>` through, because a shell is the only place an undeclared extension runs.

### Declared

A **declared** extension is one somebody registered with [`ff extension add <name>`](../reference/cli/extension-add.md). That verb runs `ff-<name> --ff-manifest`, checks the contract the manifest claims against fufu's own, and records the manifest under the user's config directory.

The record is per machine rather than per repository, since the binary is on PATH and declaring it is a decision about the machine. A declared extension is handed the same three variables, and declaring adds none.

What declaring buys is that fufu will describe it to an agent:

- the MCP tool serves its verbs, and the card names them
- `ff help <name>` and `ff explain <name>/<id>` delegate to the binary
- its briefing line rides fufu's
- its skills install beside fufu's
- the neutral agent event fans out to it
- an MCP server of its own registers beside fufu's

The card's line is `Extensions: tower (next, file, done, …)`, built from the manifest's verb list and capped in every direction: how many extensions get named, how many verbs each, and the length of the line. The card has about two thousand characters to fit in, and a registry is a person's file with nothing in fufu bounding it. An extension the cap left off is served exactly as one on the line.

Under `fufu.toolPolicy=strict` the shell refusal fires for `ff <name>` the way it fires for a builtin verb, and names the extension, because the tool is now where the verb answers. The exception is a manifest saying `undoable: false`, which the args array will not carry and the shell therefore keeps.

[`ff extension`](../reference/cli/extension.md) is itself one of the verbs the tool does not offer. The registry is the allowlist for all of the above, so an agent must not be able to write it through the tool.

`ff doctor` reports every `ff-<name>` on PATH, whether it is declared, and whether a declared one's binary still matches the manifest that was recorded.

### What a served extension owes

An extension that fufu serves owes more than a binary on PATH does. It prints fufu's envelope with `ff` as the top-level key, spells `cmd` as `<name> <verb>`, namespaces its error ids under `<name>/`, exits on the five codes above with the code agreeing with the id, and takes `--json` in last position.

Beyond that it answers a manifest handshake. It may also answer a tool-list handshake, produce a briefing line, ship skills, and subscribe to the agent event that fans out after each capture.

[Extensions](../reference/extensions.md) is the reference for building one, and types every field of all of it.

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

[Extensions](#extensions) has the two kinds, and [the reference page](../reference/extensions.md) has the contract a declared one answers in. For the repository root in any other context, `ff git rev-parse --show-toplevel` is the passthrough spelling.

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

`--at-op` is also the only place an operation id is legal outside the `ff op` family. Operations and revisions are separate address spaces, and passing one where the other belongs is a refused error rather than a convenience.

### Sessions: tagging work, and asking about it

A session is a tag on an operation, and nothing more. Set one — `--session <name>` on any invocation, or `FF_SESSION` in the environment, which is how the agent hooks stamp their own ids — and every operation recorded while it is set carries it. There is no open, no close, and nothing to clean up after a crash. Asking is a filter in the [op log's](../reference/cli/op-log.md) set language:

```console
$ ff op log 'session(flight-3)' --json | jq -c '.data.ops[]'
{"id":"wrmoxxnnywlvknmynkrnxrssypymvzyvrtynnsmn","short_id":"wrmo","kind":"capture","verb":"","summary":"manual","time":1787985391,"branch":"main","session":"flight-3","undo_of":null}
{"id":"pwknzqxqpurrvwokmqnyvuovmzumukxlpmmztywr","short_id":"pwkn","kind":"capture","verb":"","summary":"manual","time":1787985391,"branch":"main","session":"flight-3","undo_of":null}
```

`kind(capture)`, `kind(op)`, and the rest of the grammar compose the same way, so "everything agent flight-3 did that was a real verb" is one expression. Two agents interleaving in one repository stay separable forever, because the tag rides each operation rather than a range between two points.

For a consumer that wants the log pushed rather than polled, [`ff watch`](../reference/cli/watch.md) streams it: one JSON object per line as operations land, opening on a `start` line naming the tip. `--session` and `--kind` filter it, and `--all` merges every worktree into one stream.

It is a foreground process you started, not a daemon. When a trim rewrites the log under it, the stream says so and exits 1 — every id you were holding stopped resolving, so reconnect rather than carry on.
