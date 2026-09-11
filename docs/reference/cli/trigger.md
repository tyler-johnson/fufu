# ff trigger

Snapshots the working copy, now. Every ff command captures first and then goes and does something; this one captures and stops, which makes it the fastest way to force a snapshot and the natural thing to type before something risky. -m says what it is for, so a hand-taken snapshot carries its reason.

## Sources

`ff trigger <source>` means: a capture trigger fired, from this source. The other sources are machine surface rather than commands to type — claude, codex, cursor and gemini for the agent clients, shell for the prompt hook. The client invokes them with a payload on stdin.

Three rules hold for every one of them:

- They exit 0 whatever went wrong, and say nothing. FF_DEBUG=1 makes them talk.
- A source name fufu does not know exits 0 and says nothing too, which is what makes a fufu trigger safe to wire into a client fufu has never heard of.
- They never veto the action they fired on. The one veto there is, `fufu.gitPolicy strict` for raw git, is config saying so, and it travels as JSON the client may ignore rather than as an exit code.

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
