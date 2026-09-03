# The machine surface

**A script, a CI job, or an agent calling `ff` is a first-class reader: every reader takes `--json`, every failure carries a stable id, and every exit code means one thing.**

A verb computes one data model, and the human rendering and the JSON rendering are both consumers of it, never translations of each other. `--json` is therefore not the human layout re-serialized: [`ff status`](../reference/cli/status.md) crops to what an eye wants, while its JSON carries the model whole — the full change list, the [open change](../concepts/changes.md), the parent commit, the sync futures. This is what keeps the two from drifting apart, and it is why a script should parse the JSON and never the display text.

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

`landing` is `now` for where the repository stands, `undo` for each step below it, and `redo` for steps above after an undo — redo rows carry negative `distance`, so `distance` alone says how many presses in which direction. `collapsed` is how many operations the step folds together. `kind` sorts operations: `op` is a verb somebody ran, `capture` is an automatic snapshot, `note` records something that moved no tree — a publish, the log's floor. The envelope also carries `data.floor`, true when the log bottoms out at the initialized-from-observed-state entry. Every `id` here is a valid argument to the [`ff op`](../reference/cli/op.md) verbs.

## Exit codes

Five codes, one meaning each:

| code | meaning |
| --- | --- |
| 0 | done — or yes, for a command that answers a question |
| 1 | no — the command failed, or the check's answer is negative |
| 2 | the command line was wrong |
| 3 | held — a human decision is required, and the branch that held was not touched |
| 4 | contended — nothing was touched, and the same command run again is the answer |

The code follows the error id: `usage/*` errors exit 2, `held/*` errors exit 3, `ref/contended` exits 4, everything else exits 1. Exit 3 is the code git has no use for, because only a tool with land-if-clean produces the outcome: [`ff sync`](../reference/cli/sync.md) exiting 3 is a scriptable "the base moved and this needs you": the [held rewrite](../concepts/held-rewrites.md) is parked on the branch that conflicted, whatever the run landed on other branches stands, and the script should stop and surface it rather than retry. Exit 4 asks the opposite: another writer held the ref for a moment, so retry the same command, with a cap, because a lock file nobody clears gives the same answer every time. [`ff doctor`](../reference/cli/doctor.md) uses 1 as its verdict — 0 healthy, 1 findings — so CI can gate on it.

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

The envelope arrives twice, as the text content and as `structuredContent`, so a client that reads either gets the whole of it. `isError` follows the exit code: false on 0, true on anything else, and a fufu failure is a *successful* tool call carrying it, never a protocol error, because a client renders a protocol error opaquely and the `error.id` inside is what the agent has to read. The exit-code rules restate as tool rules: an id under `held/*` means nothing moved and a person is needed, so the agent stops and says so; `ref/contended` means the same call run once more. A `help` call returns the page as text with no structured content, and the verbs the tool does not offer — `git`, `update`, `watch`, `hook`, `unhook`, `mcp`, and `extension` — answer with `usage/mcp-verb-unavailable`. A declared extension is relayed the way a verb is; an undeclared one answers with `usage/mcp-extension-undeclared` and an exit naming `ff extension add <name>`, and a declared one whose manifest says `undoable: false` with `usage/mcp-extension-not-undoable`. An optional `cwd` runs the call in another directory, and `--session` on the server tags every child's operations. [Agent setup](setup.md#serve-the-verbs-as-a-tool) covers registering it.

While the server is up for a Claude Code session, `fufu.toolPolicy` (default `strict`) refuses an `ff` the agent runs through its shell tool instead, on the hook's `PreToolUse` channel. The refusal is the same JSON shape as `gitPolicy`'s, and its reason names the tool and carries the `args` to call it with, so the agent can rewrite the call without a lookup:

```json
{"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny",
 "permissionDecisionReason": "fufu.toolPolicy is strict here and the ff tool is up: call the ff tool (mcp__plugin_fufu_fufu__ff) with {\"args\":[\"status\"]} instead of running ff in the shell — load the tool's schema first if it is deferred"}}
```

`--json` is dropped from the args, since the tool adds it. What the refusal speaks to is what the tool serves: a builtin verb and a declared extension are both refused and pointed at the tool, while the seven shell-only verbs pass, and so does any `ff <name>` the tool will not serve — one nobody declared, and one declaring `undoable: false`. A declared extension's refusal names it, so an agent can tell it from the undeclared one beside it. Nothing is said at all when no server is serving that client.

## Extensions

`ff <name>` runs `ff-<name>` from PATH when no built-in verb matches, which is git's own extension model. There are two kinds of extension, and what separates them is not what one is allowed to do — it is what fufu will say about it.

An **undeclared** extension is any `ff-<name>` a PATH walk finds. fufu captures the worktree, sets three variables, and execs: `FF_REPO` is the worktree it was invoked against, absolute with symlinks resolved and forward slashes, unset outside one; `FF_CONTRACT` is the envelope version above; `FF_SESSION` is the session tag when one is set. Nothing else passes, and fufu says nothing about the verb — the tool refuses it with `usage/mcp-extension-undeclared` and an exit naming `ff extension add <name>`, it is not on the tool's card, and `ff help <name>` does not reach it. Under `fufu.toolPolicy=strict` the shell refusal lets `ff <name>` through, because a shell is the only place an undeclared extension runs.

A **declared** extension is one somebody registered with [`ff extension add <name>`](../reference/cli/extension-add.md). That verb runs `ff-<name> --ff-manifest`, checks the contract the manifest claims against fufu's own, and records the manifest under the user's config directory — per machine rather than per repository, since the binary is on PATH and declaring it is a decision about the machine. A declared extension is handed the same three variables, and declaring adds none. What declaring buys is that fufu will describe it to an agent: the MCP tool serves its verbs, the card names them, `ff help <name>` and `ff explain <name>/<id>` delegate to the binary, its briefing line rides fufu's, its skills install beside fufu's, the neutral agent event fans out to it, and an MCP server of its own registers beside fufu's. The card's line is `Extensions: tower (next, file, done, …)`, built from the manifest's verb list and capped in every direction — how many extensions get named, how many verbs each, and the length of the line — because the card has about two thousand characters to fit in and a registry is a person's file with nothing in fufu bounding it. An extension the cap left off is served exactly as one on the line. Under `fufu.toolPolicy=strict` the shell refusal fires for `ff <name>` the way it fires for a builtin verb, and names the extension, because the tool is now where the verb answers — unless the manifest says `undoable: false`, which the tool will not serve and the shell therefore keeps. [`ff extension`](../reference/cli/extension.md) is itself one of the verbs the tool does not offer: the registry is the allowlist for all of that, so an agent must not be able to write it through the tool. `ff doctor` reports every `ff-<name>` on PATH, whether it is declared, and whether a declared one's binary still matches the manifest that was recorded.

### What a served extension owes

Everything on this page, unchanged, plus five rules that make the extension's output legible as fufu's.

**The envelope's top-level key is `ff`, not the extension's name.** A served extension emits fufu's envelope verbatim — `{"ff":1,"cmd":"tower brief","data":{…}}`, with `error` in place of `data` on failure — carrying the same contract number it was handed in `FF_CONTRACT`. The key is the version of the envelope rather than a signature of whoever printed it, and the printer is already named by `cmd` and by every id it raises; a key per extension would mean a reader had to know an extension's name before it could tell a well-formed envelope from a stray line of JSON. The extension's own version is a manifest field, not a number in this slot.

**`cmd` is spelled `<name> <verb>`.** `ff tower brief` answers `"cmd":"tower brief"`, and a sub-verb extends it the way fufu's own do, `"tower bay warm"` beside `"op show"`. A call with no verb names whatever the extension's default is, as bare `ff` names `map`.

**Error ids live under `<name>/`.** `tower/flight/not-found`, never a bare `flight/not-found`: the prefix is what keeps one extension's vocabulary from colliding with fufu's or another's, and it is what routes `ff explain <name>/<id>` back to the binary that raised it. The three families that carry a rule keep their spelling inside the namespace, so what an agent already knows survives the prefix — `<name>/usage/*` is a bad command line, `<name>/held/*` means nothing moved and a person is needed, `<name>/ref/contended` means run the same call once more. An extension with no such outcome simply has no id in that family.

**The exit codes are the five above with the same meanings**, and the code follows the id: `<name>/usage/*` exits 2, `<name>/held/*` exits 3, `<name>/ref/contended` exits 4, anything else that failed exits 1, and 0 is done, or yes. The relay sets `isError` from the process's exit status alone, so a code disagreeing with its id tells the agent one thing in the envelope and another beside it.

**`--json` is accepted anywhere on the line, and stdout under it is one object on one line.** A tool call for `["tower","brief","65"]` runs `ff tower brief 65 --json`, and fufu's dispatcher strips its own globals before the exec, so the extension's argv is `brief 65 --json`: `-C` and `--session` never reach it — the directory is already changed and the session is already in the environment — and `--json` always arrives last, after every word the caller sent, which is why a flag legal only before the verb would never be seen. The relay hands stdout over as `structuredContent` only when the whole of it parses as a single envelope, so a banner, a progress line, or a pretty-printed envelope costs the agent the structured half of the reply. Anything an extension wants to say to a person goes on stderr.

### The manifest, and the `--ff-manifest` handshake

`ff-<name> --ff-manifest` prints the manifest as an ordinary envelope on one line and exits 0. It is recognized before anything else on the command line, answers outside a repository, and takes no other argument, so `ff extension add` can ask a binary what it is before it has any reason to trust it. The envelope is the shape above — `{"ff":1,"cmd":"tower --ff-manifest","data":{…}}` — and its `data` is the manifest, here pretty-printed with every optional field present:

```json
{
  "name": "tower",
  "version": "0.4.1",
  "contract": 1,
  "verbs": [
    {"name": "board", "read_only": true, "summary": "what is filed, what is moving, what is stuck"},
    {"name": "done", "read_only": false, "summary": "finish a flight"}
  ],
  "undoable": true,
  "briefing": "Work is filed as flights on a board; `ff tower` is the board.",
  "skills": ["/usr/local/share/tower/skills/tower.md"],
  "events": [{"kind": "SessionStart"}, {"kind": "BeforeTool", "matcher": "Edit|Write"}],
  "mcp": {"command": "ff", "args": ["tower", "serve", "--mcp"]}
}
```

| field | type | required | meaning |
| --- | --- | --- | --- |
| `name` | string | yes | The `<name>` in `ff-<name>`, and the namespace everything else hangs off — `cmd`, the error ids, the skills directory, the MCP server's key. Spelled by the rule the dispatcher validates a name with: ASCII alphanumeric, `-` and `_`, first character alphanumeric. A manifest claiming a name other than the binary fufu resolved is refused. |
| `version` | string | yes | The extension's own version, recorded at `add`, and what doctor compares the binary against to report drift. Opaque to fufu; nothing parses it. |
| `contract` | integer | yes | The machine-surface contract the extension speaks — the number this page calls `ff`, and the one `FF_CONTRACT` carries. A manifest naming a contract fufu does not speak is refused before anything is recorded. |
| `verbs` | array of objects | yes, non-empty | The verbs the extension answers to, in the order it wants them listed. Each carries `name`, one word; `read_only`, a boolean where false means the verb writes something; and an optional one-line `summary`. Read-only is per verb rather than per extension because an extension is usually mostly readers with a few writers, and one set of annotations on the tool cannot say that. |
| `undoable` | boolean | yes | Whether every write the extension makes is captured by fufu and taken back by `ff undo` — true only when it writes through fufu's own verbs. The one tool's annotations say that nothing it serves is destructive, which is honest only of an undoable extension, so `false` is refused on the tool with `usage/mcp-extension-not-undoable` and pointed at an MCP server of its own. The manifest still parses and the extension is still declared — it is on the card, and `ff help <name>` still delegates — and what it loses is the one tool, which is why the shell refusal lets it through. |
| `briefing` | string or `true` | no | One line for fufu's briefing to an agent. A string is the line; `true` means fufu runs `ff-<name> briefing` at print time, with the `FF_*` variables set and the event's directory, and takes its stdout. Absent means no line. Either way it is capped, and failing to produce one is silent, on [`ff trigger`](../reference/cli/trigger.md)'s doctrine. |
| `skills` | array of strings | no | Paths to the skill files the extension ships, installed under `skills/<name>/` beside fufu's own wherever a [`ff hook`](../reference/cli/hook.md) install has somewhere to put them. Absolute, or relative to the directory the binary lives in. |
| `events` | array of objects | no | Which neutral agent events the extension subscribes to. Each carries `kind` — one of `SessionStart`, `ContextStart`, `BeforeTool`, `SubagentStart`, `TurnEnd`, `SessionEnd` — and `matcher`, the tool names that subscription wants with `|` between them — `Edit|Write` — required on `BeforeTool` and refused on the rest, because every `BeforeTool` subscriber is a spawn on the agent's critical path. A name matches whole and case-sensitively against the name the client spelled, so `Edit` is `Edit` and not `NotebookEdit`. It is not a regular expression, only the alternation every client's own hook matcher already writes: fufu takes no regex engine for it, and a matcher carrying anything but tool names and `|` is refused at `ff extension add` rather than left to never fire. fufu runs `ff-<name> trigger` with the event as JSON on stdin. |
| `mcp` | object | no | An MCP server of the extension's own, registered as `mcpServers.<name>` beside fufu's when a client is hooked: `command`, a string, `args`, an array of strings, and an optional `env` object. This is where an extension wanting its own tools, annotations, or resources goes; fufu never proxies typed tools out of a manifest. |

Unknown fields are tolerated and kept, on the rule the envelope itself keeps: take fields by name. A manifest that does not parse, names a contract fufu does not speak, or claims a name other than the binary's is refused whole and nothing is recorded, because a half-declared extension is one fufu would describe and could not serve.

### The agent event, and the `trigger` reply

Every client's hook payload becomes one neutral event before fufu's pipeline reads it, and that event is what a manifest's `events` subscribes to. After the capture — never before it, because a subscriber that fails must not cost a snapshot — each declared extension that subscribed to the event — its kind, and on `BeforeTool` its tool name too — is run as `ff-<name> trigger`, in the event's own directory, with the event as one JSON object on stdin followed by EOF, and with the same three variables an extension is handed anywhere else. Subscribing adds nothing to the environment: the event is the payload. One handler covers all four clients, because the vendor's spelling is gone by the time the event reaches it.

```json
{
  "ff": 1,
  "kind": "BeforeTool",
  "source": "claude",
  "session": "5f6a3c62-1f0f-4f4e-9d0e-9a8e6b1c2d34",
  "agent": "",
  "cwd": "/repo/crates/ff-cli",
  "label": "Bash(cargo test -p ff-cli)",
  "tool": "Bash",
  "command": "cargo test -p ff-cli",
  "path": null
}
```

One line on the wire, pretty-printed here for the page's eye. Every field is always present, and a field that does not apply is `null`; nothing is omitted, so a handler indexes rather than probes.

| field | type | meaning |
| --- | --- | --- |
| `ff` | integer | The contract version this page's envelope carries, and the same number `FF_CONTRACT` holds — so a handler can check what it is reading without reaching for the environment. The event is deliberately not the envelope itself: an envelope is an answer, `data` or `error`, and an event has no failure branch to report. |
| `kind` | string | One of the six kinds a manifest subscribes to, spelled identically there and here: `SessionStart`, `ContextStart`, `BeforeTool`, `SubagentStart`, `TurnEnd`, `SessionEnd`. fufu has a seventh internally, for a well-formed event no adapter maps, which captures and fans out to nobody — it has no name in `events`, so nothing can subscribe to it. |
| `source` | string | Which trigger fired: `claude`, `codex`, `cursor` or `gemini` for the clients, `shell` for the prompt hook, `manual` for a hand-taken snapshot. The vendor's own event name went with the translation; `kind` is the meaning, and `source` is only who said it. |
| `session` | string | The client's session id, verbatim, and empty when the payload carried none. Not fufu's `FF_SESSION`, which tags operations and is set by whoever ran fufu. |
| `agent` | string | Which audience inside that session is listening: a subagent's id, or empty for the main thread. A subagent inherits the parent's session id and was told none of what the parent was told, so this is the field that tells the two apart. Empty is a value here, not an absence. |
| `cwd` | string | The directory the client named, which is what fufu discovered the repository from. `FF_REPO` is the worktree that discovery landed on, so the two differ whenever the event fired below the root, and `cwd` is the more specific of them. |
| `label` | string | The detail fufu put in the snapshot's subject: `Bash(cargo test)`, `Edit(src/lib.rs)`, `prompt "…"`, `event PostToolUse`. Cut to a subject line and rendered with paths relative to the worktree, which makes it good to show and bad to parse — the fields below are the ones to read. |
| `tool` | string or null | The tool's name as the client spelled it, on `BeforeTool`, and `null` on every other kind. This is the string a subscription's `matcher` is tested against, so an event carrying no tool name — a shell prompt, a hand-taken snapshot — matches nothing and spawns nobody. |
| `command` | string or null | The tool's shell command, verbatim and untruncated, and `null` for a tool that carried none. The label's copy is cut to a subject line, and a classifier reading a cut command line would be reading a different command. |
| `path` | string or null | The file the tool named, by whichever of the field names that client uses for it, relative to the worktree when it is inside one and absolute when it is not. `null` when the tool named none. |

Six of those are fufu's neutral event written down rather than anything new: `kind`, `session`, `agent`, `cwd`, `label` and `command` are the fields the capture pipeline already consumes. `source` is what fufu already puts in the snapshot's provenance. `tool` and `path` are the two the fan-out adds, and they are added because a subscriber has to be able to act on a tool call it was woken for: `tool` is the name the required `BeforeTool` matcher is tested against, and a handler that could only read `Edit(src/lib.rs)` out of the label would be parsing a subject line to find out which file moved.

**The reply is an envelope, and fufu reads one field of it.** stdout is the shape every served extension prints, with `cmd` spelled `<name> trigger`:

```json
{"ff":1,"cmd":"tower trigger","data":{"context":"tower: flight #73 is in progress on this branch."}}
```

`context` is text to put in front of the agent, and in this cut it is the whole of what an extension may say. It is merged into the one reply the client already gets — fufu's own briefing and corrections first, then each subscriber in the registry's order, joined by newlines and rendered once through whatever channel that client has for that kind. Where the client has no channel, nothing is printed and every subscriber still ran, exactly as with fufu's own lines. Most events are ones a subscriber has nothing to say about, and printing nothing at all is the right answer to those. Any other field in `data` is ignored rather than refused, so a later contract can define one without breaking a handler written against this one.

**`ff-<name> trigger` exits 0, whatever happened.** It is `ff trigger`'s doctrine inherited whole, and for the same reason: the handler rides an event whose job is a snapshot, and an extension having a bad day must cost the agent nothing. A nonzero exit is not a failure fufu reports; it is a reply fufu drops.

**It is silent.** stdout is read as the reply and nothing else, so anything that is not one parsable envelope — a banner, a progress line, a pretty-printed envelope, an envelope carrying `error` — is a subscriber with nothing to say. stderr is not shown to anyone. `FF_DEBUG=1` is where both go when something needs to be seen, beside fufu's own complaint.

**It is time-boxed by fufu.** The budget belongs to fufu rather than to the extension, because the agent's turn is not the place to wait on somebody else's network call. A subscriber that has not answered in time contributes nothing and the event carries on without it, with no message to the agent about the one that was late. An extension with real work to do on an event should record the event and return.

**It injects context and it cannot veto.** No field of the reply refuses the action the event fired on. The two refusals on this page remain the two there are, `fufu.gitPolicy strict` and `fufu.toolPolicy strict`, both of them config saying so in a repository somebody configured; neither is reachable from a manifest, and an extension that wants an action stopped has nowhere to say it here. A subscription buys being told, and being able to answer with words.

## Piped output never pages

The log family — [`ff log`](../reference/cli/log.md), [`ff evolog`](../reference/cli/evolog.md), `ff op log` — pages on a terminal, git-style: `fufu.pager`, then `FF_PAGER`, then `PAGER`, then `less`. A pager spawns only when stdout is a real TTY and the view is human. Piped output and `--json` never page, so a script never needs `| cat`, never inherits a hung `less`, and never sees pager chrome in its bytes. Color follows the same discipline: ANSI is emitted only where a terminal will read it, and `NO_COLOR` is honored, so piped human output is plain text.

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

An extension gets the answers handed to it: `ff <name>` runs `ff-<name>` from PATH when no built-in verb matches, and the child inherits `FF_REPO` (the worktree it was invoked against, unset outside one), `FF_CONTRACT` (the envelope version it is about to parse), and `FF_SESSION` (the session tag when one is set) — [Extensions](#extensions) has the two kinds and the contract a declared one answers in. For the repository root in any other context, `ff git rev-parse --show-toplevel` is the passthrough spelling.

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
