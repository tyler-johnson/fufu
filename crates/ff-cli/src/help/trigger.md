Snapshots the working copy, now. Every ff command captures first and then goes and does something; this one captures and stops, which makes it the fastest way to force a snapshot and the natural thing to type before something risky. -m says what it is for, so a hand-taken snapshot carries its reason.

### Sources

`ff trigger <source>` means: a capture trigger fired, from this source. The other sources are machine surface rather than commands to type — claude, codex, cursor and gemini for the agent clients, shell for the prompt hook. The client invokes them with a payload on stdin.

Three rules hold for every one of them:

- They exit 0 whatever went wrong, and say nothing. FF_DEBUG=1 makes them talk.
- A source name fufu does not know exits 0 and says nothing too, which is what makes a fufu trigger safe to wire into a client fufu has never heard of.
- They never veto the action they fired on. The two vetoes there are — `fufu.gitPolicy strict` for raw git, and `fufu.toolPolicy strict` for `ff` in the shell while the `ff` tool is up — are config saying so, and each travels as JSON the client may ignore rather than as an exit code.

### Extensions

Every one of those events reaches the declared extensions that subscribed to its kind, after the capture and never before it, and whatever `context` their replies carry is merged into the one reply the client was already getting.

A subscriber inherits this page's doctrine whole: it exits 0 whatever happened, it is silent, and it cannot veto. The time box is fufu's, half a second shared across the whole fan-out. Which events an extension asks for is its manifest's business; `ff extension` is the verb that declares one.

## Examples

```
ff trigger                     snapshot now
ff trigger -m "before this"    and say why it was taken
ff op log                      the snapshot you just took
ff restore --all --at 2h       what the snapshots are for
```
