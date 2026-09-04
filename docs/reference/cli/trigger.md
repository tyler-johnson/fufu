# ff trigger

Snapshots the working tree, now. Every ff command captures first and then goes and does something; this one captures and stops, which makes it the fastest way to force a snapshot and the natural thing to type before something risky. -m says what it is for, so a hand-taken snapshot carries its reason.

`ff trigger <source>` means: a capture trigger fired, from this source. The other sources are machine surface, not commands to type — claude, codex, cursor, gemini for the agent clients, and shell for the prompt hook. Those are invoked by the client with a payload on stdin, they always exit 0 whatever went wrong, and they never veto the action they fired on on their own judgment.

The two vetoes there are, `fufu.gitPolicy strict` for raw git and `fufu.toolPolicy strict` for `ff` in the shell while the `ff` tool is up, are config saying so, and each travels as JSON the client may ignore rather than as an exit code.

Their failures are silent by design; FF_DEBUG=1 makes them talk. A source name fufu does not know exits 0 and says nothing, which is what makes a fufu trigger safe to wire into a client fufu has never heard of.

Every one of those events reaches the declared extensions that subscribed to its kind, after the capture and never before it. Each is run as `ff-<name> trigger` in the event's own directory, with the event as one JSON object on stdin, and whatever `context` its reply carries is merged into the one reply the client was already getting — fufu's lines first, then each subscriber in the order it was declared.

A subscriber inherits this page's doctrine whole: it exits 0 whatever happened, it is silent, and it cannot veto. The time box is fufu's, half a second shared across the whole fan-out, because an agent pays for it on every event and on a tool call it pays on the critical path.

## Usage

```
Usage: ff trigger [OPTIONS] [source]

Arguments:
  [source]
          The source; absent or `manual` is the hand-taken snapshot

Options:
  -m <msg>
          Say what this snapshot is for

      --json
          Emit machine-readable JSON

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Examples

```
ff trigger                     snapshot now
ff trigger -m "before this"    and say why it was taken
ff op log                      the snapshot you just took
ff restore --all --at 2h       what the snapshots are for
```
