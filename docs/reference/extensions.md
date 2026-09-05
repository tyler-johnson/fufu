# Extensions

`ff <name>` runs `ff-<name>` from PATH whenever no built-in verb matches. That is git's extension model, and it needs nothing from you but an executable on PATH with the right filename — a shell script, a Python file, a compiled binary, anything the operating system will start.

This page is about the other half: **declaring** an extension, which is how fufu learns enough about your binary to describe it to an agent. Read it if you are building one. It assumes [the machine surface](../agents/machine-surface.md), which is the contract your output has to speak.

## The two kinds

An **undeclared** extension is any `ff-<name>` on PATH. fufu snapshots the worktree, sets three environment variables, and runs it. That is the whole relationship: fufu found a filename, so a filename is all it knows. The MCP tool will not relay it, `ff help <name>` does not reach it, and it is not on the tool's card.

A **declared** extension is one somebody ran [`ff extension add <name>`](cli/extension-add.md) on. fufu asked the binary for a manifest, checked it, and recorded it. Declaring buys no new capability and no new environment — a declared extension runs exactly as it did before. What it buys is that fufu will now *talk about* the extension:

- The [`ff mcp`](cli/mcp.md) tool relays its verbs, so an agent can call them.
- The tool's card names the extension and some of its verbs.
- `ff help <name>` and [`ff explain <name>/<id>`](cli/explain.md) delegate to the binary.
- A line from the extension rides fufu's briefing to an agent.
- Its skills install beside fufu's on [`ff hook`](cli/hook.md).
- Agent events fan out to it.
- MCP tools of its own are served beside fufu's, and an MCP server of its own is registered beside fufu's.
- [`ff doctor`](cli/doctor.md) reports on it.

The record lives under your config directory, not in a repository, because the binary is on PATH and declaring it is a decision about the machine. Declaring is also the one thing an agent cannot do through the MCP tool: the list is the allowlist for everything above, so putting a name on it stays a person's gesture.

## The smallest extension that works

Save this as `ff-hello` somewhere on PATH and make it executable:

```sh
#!/bin/sh
# ff-hello — `ff hello greet` finds it.
set -eu

if [ "${1-}" = --ff-manifest ]; then
  echo '{"ff":1,"cmd":"hello --ff-manifest","data":{"name":"hello","version":"0.1.0","contract":1,"undoable":true,"verbs":[{"name":"greet","read_only":true,"summary":"say where fufu put us"}]}}'
  exit 0
fi

case "$*" in
"greet")
  echo "hello from ${FF_REPO:-outside a repository}"
  ;;
"greet --json")
  echo "{\"ff\":1,\"cmd\":\"hello greet\",\"data\":{\"repo\":\"${FF_REPO-}\"}}"
  ;;
*"--json")
  echo '{"ff":1,"cmd":"hello","error":{"id":"hello/usage/unknown-verb","message":"ff hello takes one verb: greet","exits":["ff hello greet"]}}'
  exit 2
  ;;
*)
  echo "ff hello takes one verb: greet" >&2
  exit 2
  ;;
esac
```

It runs undeclared straight away, because that is all an undeclared extension is:

```console
$ ff hello greet
hello from /tmp/project
```

Declaring it takes one command, and asks the binary that `--ff-manifest` question:

```console
$ ff extension add hello
declared hello 0.1.0 from /usr/local/bin/ff-hello
  its verbs: greet
undo: ff extension remove hello
$ ff extension
hello  0.1.0  greet
```

From here an agent can call `ff hello greet` through fufu's MCP tool, and gets the envelope back:

```console
$ ff hello greet --json
{"ff":1,"cmd":"hello greet","data":{"repo":"/tmp/project"}}
```

Everything below is detail on that shape and on the optional surfaces declaring opens up.

## What fufu hands your binary

Three environment variables, on every invocation, declared or not. Declaring adds none.

| variable | value |
| --- | --- |
| `FF_REPO` | The worktree the command was invoked against — absolute, symlinks resolved, forward slashes on every platform. **Unset** outside a repository, so treat an unset value as a real case rather than an error. |
| `FF_CONTRACT` | The machine-surface contract version, currently `1`. Put this number in the envelopes you print. |
| `FF_SESSION` | The session tag, when whoever ran fufu set one. Unset otherwise. |

Nothing else is passed. The current directory is where the caller was; if fufu was given `-C`, the directory is already changed before your binary starts.

For anything else about the repository, run git — [`ff git rev-parse --show-toplevel`](cli/git.md) and friends work fine from inside an extension.

## What fufu asks your binary

Every one of these is your binary, started by fufu with these arguments. The first two reach any `ff-<name>` on PATH — `--ff-manifest` is how `ff extension add` asks a binary what it is before it trusts it. The rest are only asked of a *declared* extension.

| fufu runs | when | you print |
| --- | --- | --- |
| `ff-<name> <verb> …` | someone typed `ff <name> <verb> …` | whatever the verb does |
| `ff-<name> --ff-manifest` | `ff extension add`, `ff doctor` | the manifest envelope, exit 0 |
| `ff-<name> --ff-tools` | `ff mcp` starts, `ff doctor` | the tool descriptors, exit 0 |
| `ff-<name> help` | `ff help <name>` | your help page, on stdout |
| `ff-<name> explain <id>` | `ff explain <name>/<id>` | prose for that id — the `<name>/` prefix is stripped before it reaches you |
| `ff-<name> briefing` | a briefing is built, and your manifest says `briefing: true` | one line |
| `ff-<name> trigger` | an event you subscribed to fired, with the event JSON on stdin | an envelope carrying `data.context`, exit 0 |

## Speaking fufu's contract

An agent reading your output should not have to know it left fufu. Five rules do that.

**Print fufu's envelope, with `ff` as the top-level key.** Not your extension's name:

```json
{"ff":1,"cmd":"hello greet","data":{"repo":"/tmp/project"}}
```

`ff` is the contract version — the number `FF_CONTRACT` handed you. `data` on success, `error` on failure, never both. Your own version belongs in the manifest, not here. The key names the *envelope's* version, so that a reader can tell a well-formed envelope from a stray line of JSON without first knowing who printed it.

**Spell `cmd` as `<name> <verb>`.** `ff hello greet` answers `"cmd":"hello greet"`. A sub-verb extends it the same way — `"tower bay warm"`. A call with no verb names whatever your default is.

**Prefix your error ids with `<name>/`.** `hello/usage/unknown-verb`, never a bare `usage/unknown-verb`. The prefix keeps your vocabulary from colliding with fufu's, and it is what routes `ff explain hello/usage/unknown-verb` back to your binary. Three families carry a meaning an agent already knows, so keep their spelling inside your namespace:

| id | meaning |
| --- | --- |
| `<name>/usage/*` | the command line was wrong |
| `<name>/held/*` | nothing moved, and a person has to decide something |
| `<name>/ref/contended` | nothing moved, and running the same call again is the answer |

If you have no such outcome, you simply have no id in that family.

**Exit with fufu's codes, and make the code agree with the id.** `<name>/usage/*` exits 2, `<name>/held/*` exits 3, `<name>/ref/contended` exits 4, any other failure exits 1, and 0 is done — or yes, for a verb that answers a question. The MCP relay sets `isError` from the exit status alone, so a code that disagrees with its id tells the agent one thing in the envelope and the opposite beside it.

**Accept `--json` anywhere on the line, and print one object on one line under it.** fufu appends `--json` *last*, after every word the caller sent, so a flag that is only legal before the verb will never be seen. fufu also strips its own globals before running you: `-C` and `--session` never reach your argv.

Under `--json`, stdout is the envelope and nothing else. A banner, a progress line, or a pretty-printed envelope costs the agent the structured half of the reply, because the relay only hands stdout over as `structuredContent` when the whole of it parses as one envelope. Anything you want to say to a person goes on stderr.

## The manifest

`ff-<name> --ff-manifest` prints the manifest as an ordinary envelope on one line and exits 0.

Recognize the flag before anything else on your command line, answer it outside a repository, and take no other argument. `ff extension add` uses it to ask a binary what it is before it has any reason to trust it, and hands down nothing but `FF_NONINTERACTIVE=1`. This handshake is **not** time-boxed — a person typed the verb and can interrupt it.

Here is one with every optional field present, pretty-printed for the page:

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
  "skills": ["tower", "tower-plan", "tower-loop"],
  "events": [{"kind": "SessionStart"}, {"kind": "BeforeTool", "matcher": "Edit|Write"}],
  "tools": true,
  "mcp": {"command": "ff", "args": ["tower", "serve", "--mcp"]}
}
```

| field | type | required | meaning |
| --- | --- | --- | --- |
| `name` | string | yes | The `<name>` in `ff-<name>`, and the namespace everything else hangs off — `cmd`, your error ids, your skills directory, your MCP server's key. ASCII alphanumeric, `-` and `_`, first character alphanumeric. It must match the binary fufu resolved. |
| `version` | string | yes | Your own version. fufu records it and never parses it; `ff doctor` compares the binary against it to report drift. |
| `contract` | integer | yes | The machine-surface contract you speak — the number `FF_CONTRACT` carries, currently `1`. A manifest naming a contract fufu does not speak is refused. |
| `verbs` | array of objects | yes, non-empty | The verbs you answer to, in the order you want them listed. Each carries `name`, one word; `read_only`, where false means the verb writes something; and an optional one-line `summary`. Read-only is per verb because most extensions are mostly readers with a few writers. |
| `undoable` | boolean | yes | Whether [`ff undo`](cli/undo.md) takes back every write you make. See [below](#undoable-and-what-false-costs). |
| `briefing` | string or `true` | no | One line for fufu's briefing to an agent. See [below](#optional-a-briefing-line). |
| `skills` | array of strings | no | The names of skills you ship, each produced by `--ff-skill`. See [below](#optional-skills). |
| `events` | array of objects | no | Agent events you subscribe to. See [below](#optional-agent-events). |
| `tools` | `true` | no | Whether you produce MCP tool descriptors. See [below](#optional-mcp-tools). |
| `mcp` | object | no | An MCP server of your own. See [below](#optional-an-mcp-server-of-your-own). |

Unknown fields are tolerated and kept, so a later contract can add one without breaking you. A manifest that does not parse, names a contract fufu does not speak, or claims a name other than the binary's is refused whole and nothing is recorded — a half-declared extension is one fufu would describe and could not serve.

### `undoable`, and what `false` costs

Say `true` only when every write you make goes through fufu's own verbs, so that `ff undo` takes all of it back.

The reason it matters: fufu's MCP tool is a single tool, `ff`, whose input is an args array, and it carries one set of annotations over everything that array relays. Those annotations say nothing relayed is destructive. That is honest only of an undoable extension, so an extension declaring `undoable: false` is refused on that route with `usage/mcp-extension-not-undoable`.

What `false` costs is that one route, and nothing else. You are still declared, still on the card, `ff help <name>` still delegates, and any [MCP tools you produce](#optional-mcp-tools) are still listed and called — they carry annotations of their own and are honest without the blanket promise. The `fufu.toolPolicy=strict` shell refusal also lets `ff <name>` through for you, so a shell is always somewhere the verb can still run.

Both routes stand together. An `undoable: true` extension that also produces tools gets its verbs relayed through the args array *and* its tools listed beside them. You never give up one to gain the other.

## Declaring it, checking it, taking it back

Three verbs cover the whole of it: `ff extension add`, [`ff extension list`](cli/extension-list.md) (bare [`ff extension`](cli/extension.md) is the same list), and [`ff extension remove`](cli/extension-remove.md).

```console
$ ff extension add hello        # ask ff-hello what it is, and record it
$ ff extension                  # what this machine declares
$ ff extension list --json      # the manifests as they were recorded
$ ff extension remove hello     # fufu stops describing it; ff-hello still runs
```

Declaring the same name again replaces the record and keeps its place in the order, which is the order subscribers are fanned out in and the order the card names extensions in. Upgrading a binary is not a reordering.

What gets recorded is the manifest as it was read, unknown fields and all, plus the path the walk landed on and the time. The path is evidence, not a route — dispatch stays a fresh PATH walk, so a binary that moves is still found.

`ff undo` does not reach any of this: the record lives outside every repository, so the way back is `ff extension add <name>` again.

`ff doctor` is where you check your work, and the [Doctor page](doctor.md) reads its rows. It runs the handshakes for real rather than trusting the record, so it costs one spawn per declared extension and a second one for each that promised tools:

```console
$ ff doctor
  ok    hello          0.1.0 matches ff-hello on PATH
  info  extensions     1 on PATH, undeclared: ff-tower (ff extension add <name> declares one)
```

It is the one place a failed tools handshake shows up, because everywhere else fufu stays silent about it.

## Optional: a briefing line

fufu puts a short notice in front of an agent at the start of a session. A declared extension may add **one line** to it, and one line is the whole of what it may add.

Set `briefing` to a string and that string is the line. Set it to `true` and fufu runs `ff-<name> briefing` when the briefing is built — in the event's own directory, with the three variables — and takes your stdout. Use `true` when the line depends on the repository or on your own state; use a string when it does not.

Either way the line is trimmed to one line and capped at 240 characters, and a line past the cap is **dropped whole** rather than cut, because half a sentence is still prose the agent reads as instructions.

Failing to produce a line costs nothing and says nothing. A binary that has left PATH, will not start, exits nonzero, prints something that is not a line, or is still thinking when fufu's time box runs out all come to the same outcome: no line, and a briefing that is exactly what it would have been. `FF_DEBUG=1` is where the reason goes when you want to see it.

## Optional: skills

A skill is a manual a client loads when the situation calls for it, and one a person can type: `/fufu:tower-plan` in Claude Code, `$tower-plan` in Codex. `skills` is the list of the ones you ship, **by name**, and `ff hook` asks your binary for each one's files at install time. Wherever an install has somewhere to put skills, each of yours lands whole in a directory of its own beside fufu's.

Names rather than paths, on the rule `tools` draws: a path written into the manifest names a file that a binary shipped alone out of a tarball does not have beside it, and the markdown embedded in that binary is where your build already put the text.

A skill's name is ASCII letters and digits, `-` and `_`, and is either your extension's name or starts with it and a dash: `tower`, `tower-plan`, `tower-loop`. Every declared extension's skills share one directory beside fufu's, so a bare `plan` would be whichever extension wrote it last. A manifest naming a skill outside its own namespace is refused with `extension/bad-manifest`.

### `--ff-skill`

`--ff-skill <skill>` behaves exactly like `--ff-manifest` — recognized before anything else on the command line, answers outside a repository, prints one envelope on one line, exits 0 — and takes the skill's name as its one argument. Nothing is handed down but `FF_NONINTERACTIVE=1`.

It is **not** time-boxed. The callers are `ff hook` and `ff hook --skill`, verbs a person typed and can interrupt, and nothing asks a binary for a skill with nobody there.

The envelope's `data` is the files the skill is made of, pretty-printed here:

```json
{
  "files": [
    {"path": "SKILL.md", "content": "---\nname: tower-plan\ndescription: Load the board with a person.\ndisable-model-invocation: true\n---\n# Plan\n…"},
    {"path": "scripts/run.sh", "content": "#!/bin/sh\nff tower\n"}
  ]
}
```

| field | type | required | meaning |
| --- | --- | --- | --- |
| `files` | array of objects | yes, non-empty | The files, each carrying `path` and `content`. |
| `path` | string | yes | Where the file lands under the skill's directory. Relative, normal components only: no `..`, no leading `.`, not absolute. Exactly one of them is `SKILL.md` at the root, and no two are the same. |
| `content` | string | yes | The file's text. |

The files weigh at most 8 MiB together. The skill is refused whole rather than in part, with `extension/bad-skill`, because a skill a client loads half of is one its `SKILL.md` describes and its scripts cannot back. A skill you have no answer for gets an error envelope, `<name>/skill/unknown` or the like; fufu reads only that it is an error, and refuses with `extension/skill-failed`.

A skill that does not come back whole is left out of the install and said, one dim line naming the skill and the reason. It never fails the install, and it costs no other skill its place.

### Where a skill lands

Claude Code takes each skill inside fufu's plugin, at `~/.claude/skills/fufu/skills/<skill>/`, and a person types it as `/fufu:<skill>`. The plugin's `skills/` directory is wholly fufu's, so a rerun of `ff hook claude` sweeps it: a skill of an extension no longer declared goes.

Codex takes each skill at `~/.codex/skills/<skill>/`, and a person mentions it as `$<skill>`. That directory is shared with everything else Codex has, so nothing sweeps it: `ff extension remove` before `ff unhook codex` leaves the extension's skills behind.

Cursor and Gemini read no skills directory and get nothing.

Both clients want `name` and `description` in `SKILL.md`'s front matter, and `disable-model-invocation: true` keeps a skill for people to type rather than one the model may load on its own.

Rerunning `ff hook` refreshes every skill from the binary. `ff extension remove` stops the *next* install from carrying them; `ff hook --skill <skill>` prints any declared skill's `SKILL.md` without installing anything.

## Optional: agent events

Every client's hook payload becomes one neutral event before fufu's pipeline reads it, and that event is what you subscribe to. One handler covers all four clients, because the vendor's spelling is gone by the time the event reaches you.

Subscribe in the manifest:

```json
"events": [{"kind": "SessionStart"}, {"kind": "BeforeTool", "matcher": "Edit|Write"}]
```

`kind` is one of `SessionStart`, `ContextStart`, `BeforeTool`, `SubagentStart`, `TurnEnd`, `SessionEnd`.

`matcher` is the tool names that subscription wants, with `|` between them. It is **required on `BeforeTool` and refused on every other kind**, because every `BeforeTool` subscriber is a process spawn on the agent's critical path.

It is not a regular expression — only the alternation every client's own hook matcher already writes. A name matches whole and case-sensitively, so `Edit` is `Edit` and not `NotebookEdit`. A matcher carrying anything but tool names and `|` is refused at `ff extension add`, rather than left to quietly never fire.

When the event fires, fufu runs `ff-<name> trigger` **after** the capture — never before it, because a subscriber that fails must not cost a snapshot — in the event's own directory, with the three variables, and the event as one JSON object on stdin followed by EOF.

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

One line on the wire, pretty-printed here. Every field is always present, and a field that does not apply is `null` — nothing is omitted, so you can index rather than probe.

| field | type | meaning |
| --- | --- | --- |
| `ff` | integer | The contract version, the same number `FF_CONTRACT` holds. This is an event and not an envelope: it has no failure branch to report. |
| `kind` | string | One of the six above, spelled identically here and in `events`. |
| `source` | string | Who fired: `claude`, `codex`, `cursor`, `gemini`, `shell` for the prompt hook, `manual` for a hand-taken snapshot. |
| `session` | string | The client's session id, verbatim, and empty when the payload carried none. Not `FF_SESSION`, which is fufu's own tag. |
| `agent` | string | Which audience inside that session is listening: a subagent's id, or empty for the main thread. A subagent inherits the parent's session id and was told none of what the parent was told, so this is the field that tells them apart. |
| `cwd` | string | The directory the client named, which is what fufu discovered the repository from. `FF_REPO` is where that discovery landed, so the two differ whenever the event fired below the root. |
| `label` | string | What fufu put in the snapshot's subject — `Edit(src/lib.rs)`, `prompt "…"`. Good to show, bad to parse; read the fields below instead. |
| `tool` | string or null | The tool's name as the client spelled it, on `BeforeTool`, and `null` on every other kind. This is what `matcher` is tested against. |
| `command` | string or null | The tool's shell command, verbatim and untruncated. `null` for a tool that carried none. |
| `path` | string or null | The file the tool named, relative to the worktree when it is inside one. `null` when the tool named none. |

Reply with an envelope, and fufu reads exactly one field of it:

```json
{"ff":1,"cmd":"hello trigger","data":{"context":"hello: 3 flights are in progress on this branch."}}
```

`context` is text to put in front of the agent, and in this contract it is the whole of what you may say. Any other field in `data` is ignored rather than refused.

The text is merged into the one reply the client already gets — fufu's own lines first, then each subscriber in the order the extensions were declared. Where the client has no channel for that kind of event, nothing is printed and every subscriber still ran.

Four rules govern the handler, and they exist because it rides an event whose real job is a snapshot:

- **Exit 0, whatever happened.** A nonzero exit is not a failure fufu reports; it is a reply fufu drops.
- **Be silent.** stdout is read as the reply and nothing else, so a banner, a progress line, a pretty-printed envelope, or an envelope carrying `error` all read as a subscriber with nothing to say. stderr is shown to nobody. `FF_DEBUG=1` is where both go when something needs to be seen.
- **Be quick.** fufu time-boxes the fan-out, splitting one budget across the subscribers it has not asked yet. A subscriber that has not answered in time contributes nothing and the event carries on, with no message to the agent about the one that was late. If you have real work to do on an event, record the event and return.
- **You cannot veto.** No field of the reply refuses the action the event fired on. A subscription buys being told, and being able to answer with words.

Most events are ones you have nothing to say about, and printing nothing at all is the right answer to those.

## Optional: MCP tools

Beside relaying your verbs through its one `ff` tool, fufu can serve **typed tools** of your own — each with its own name, description, input schema, and annotations, exactly as any MCP server's tools have.

Say `"tools": true` in the manifest. That is a promise rather than a list: fufu then asks `ff-<name> --ff-tools` for the list itself. Writing the list into the manifest would be a second spelling of your own CLI, kept in step by hand and stale the moment the binary moved on; generating it from the definitions your flags already come from makes drift impossible rather than policed. `briefing: true` draws the same rule.

### `--ff-tools`

`--ff-tools` behaves exactly like `--ff-manifest` — recognized before anything else on the command line, answers outside a repository, takes no other argument, prints one envelope, exits 0 — with one difference. **It is time-boxed, at about a second.**

`ff extension add` and `ff doctor` are verbs a person typed and can interrupt; this one is asked by a server starting up with nobody in front of it, where a binary that hangs would hang the server before it served anything.

Nothing is handed down but `FF_NONINTERACTIVE=1`: you need neither the repository nor the contract to say what tools you have.

The envelope's `data` is an array of descriptors, pretty-printed here:

```json
[
  {
    "name": "board",
    "description": "What is filed, what is moving, and what is stuck.",
    "inputSchema": {
      "type": "object",
      "properties": {"branch": {"type": "string"}},
      "additionalProperties": false
    },
    "annotations": {"readOnlyHint": true, "destructiveHint": false}
  },
  {
    "name": "file",
    "description": "File a flight on the board.",
    "inputSchema": {
      "type": "object",
      "properties": {"title": {"type": "string"}},
      "required": ["title"],
      "positional": ["title"]
    },
    "annotations": {"readOnlyHint": false, "destructiveHint": false, "idempotentHint": false}
  }
]
```

| field | type | required | meaning |
| --- | --- | --- | --- |
| `name` | string | yes | What the tool is called, **bare** — `board`, not `tower__board`. Namespacing is fufu's job, and a name that arrived namespaced would be namespaced twice. ASCII letters and digits, `-` and `_`. Two descriptors may not share one. |
| `description` | string | yes | What the tool does, which is the whole of what an agent reads before calling it. Required and non-empty, where MCP leaves it optional. |
| `inputSchema` | object | yes | The JSON Schema the call's arguments are shaped by. `"type": "object"`, since a call's arguments arrive as an object. |
| `annotations` | object | yes | MCP's own hints. `readOnlyHint` and `destructiveHint` are both required; `idempotentHint`, `openWorldHint` and `title` are optional. A tool claiming to be read-only and destructive at once is refused. |

The field names are MCP's camel case rather than the manifest's snake case, because a descriptor is MCP's object — if you already have one, copy it across. Unknown fields are tolerated and dropped, since nothing records a descriptor and there is no round trip for one to survive.

Both hints are required because a produced tool is offered on what it says about itself, which is what lets a non-undoable extension serve tools at all.

The list is refused whole rather than in part: a tool an agent can call by a name that is sometimes there is worse than one it cannot call at all. A list that promised tools may not come back empty.

### How a call becomes a command line

**The client sees `<extension>__<tool>`** — `tower__board` for tower's `board` — which is the shape MCP itself uses when a client prefixes a server's tools. Two extensions cannot collide by both producing a `list`. An extension name may itself carry `_`, so two namespaced names can still meet; the first extension declared keeps the name and the later tool is not listed. fufu's own tool keeps its bare `ff`.

**A tool's bare name is the verb it calls.** `tower__board` with `{"branch": "main"}` runs `ff tower board --branch main --json`.

**Every property is spelled as a long option, verbatim** — no case or underscore translation, because you generated the schema from the definitions your flags come from.

| JSON value | command line |
| --- | --- |
| `true` | `--key` |
| `false`, `null` | nothing at all |
| a string or number | `--key <value>` |
| an array | the flag repeated once per item |
| an object, or an array inside an array | refused as a protocol error before anything runs |

`inputSchema` may carry one keyword of fufu's own beside JSON Schema's: **`positional`**, an array of property names spelled as bare words, in that array's order, before every option. A positional left out ends the line there rather than shifting the words after it onto the wrong argument.

A call arriving through a produced tool is an ordinary invocation of your binary, so [the five rules](#speaking-fufus-contract) hold unchanged — the envelope key, `cmd`, the id prefix, the exit codes, and `--json` last on the line.

**The list is asked for once, when the server starts, and held for the life of the connection.** What was advertised at handshake is what answers until the client closes, so restarting the client is what picks up an edited extension.

A failed handshake costs the agent nothing and says nothing: fufu serves its own tool, your verbs are relayed exactly as they were, and what is lost is the tools you promised. `ff doctor` is where that shows.

## Optional: an MCP server of your own

```json
"mcp": {"command": "ff", "args": ["tower", "serve", "--mcp"], "env": {"TOWER_MODE": "board"}}
```

When a client is hooked, fufu registers this as `mcpServers.<name>` beside its own. `command` is a string, `args` an array of strings, `env` an optional object.

This is for what only a live process can hold: resources a client attaches and re-reads, a notification when state moves, a subscription, session identity across calls, a warm cache. If all you have is typed tools, use [`tools`](#optional-mcp-tools) instead — it needs no process of your own and no separate registration.

## What fufu refuses, and when

These come from declaring — where a refusal records nothing — and from the handshakes fufu makes afterwards.

| id | what happened |
| --- | --- |
| `extension/not-found` | no `ff-<name>` on PATH to ask |
| `extension/handshake-failed` | the binary did not answer `--ff-manifest` |
| `extension/bad-manifest` | it answered, and fufu cannot read the manifest |
| `extension/name-mismatch` | the manifest claims a name other than the binary's |
| `extension/unsupported-contract` | it speaks a contract this fufu does not |
| `extension/not-declared` | nothing is declared under that name |
| `extension/tools-failed` | the binary did not answer `--ff-tools` |
| `extension/bad-tools` | it answered, and fufu cannot read the list |
| `extension/delegate-failed` | `help`, `explain`, or `briefing` went unanswered |
| `extension/registry-unreadable` | the record file is there and does not read as one |
| `extension/registry-unwritable` | there is nowhere to record the declaration |

Two more come from the MCP tool rather than from declaring. `usage/mcp-extension-undeclared` means an agent called an extension nobody declared, and its exits name `ff extension add <name>`. `usage/mcp-extension-not-undoable` means the extension is declared and said `undoable: false`, so the args array will not carry it.

[The error id index](errors.md) lists all of these with their exit codes, and `ff explain <id>` prints prose for any one of them.

## A checklist

Before you declare:

- `ff-<name>` is executable and on PATH.
- `--ff-manifest` is recognized before anything else on your command line, works outside a repository, prints one envelope on one line, and exits 0.
- The manifest's `name` matches the binary, its `contract` is `1`, and `verbs` is non-empty.
- `undoable` is honest.
- Every verb takes `--json` in last position and prints one envelope on one line under it.
- Every error id starts with `<name>/`, and the exit code agrees with it.
- Human output goes to stderr under `--json`.

Then `ff extension add <name>`, and `ff doctor` to confirm what fufu sees.
