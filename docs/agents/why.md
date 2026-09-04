# Why agents want fufu

fufu's pitch is version control for humans and agents. This page argues the second half: what goes wrong when an agent drives plain git, and what fufu changes about it.

## The failure mode

An agent with shell access and `git` on PATH is one confident `git reset --hard` from destroying an afternoon.

Git's destructive commands — `reset --hard`, `checkout .`, `clean -fd`, `restore <file>` — assume the person typing them has already weighed what they discard. An agent supplies the conviction without the weighing. When its model of the tree diverges from the tree, discarding the tree is the fix it reaches for, and it reaches without hesitating.

The exposure is wider than the dramatic commands:

- **The valuable state is uncommitted.** An agent edits at machine rate and may commit nothing for an hour, so at any given moment the work that matters is uncommitted — precisely the state git protects least.
- **The reflog records refs, not trees.** It says where branch pointers moved and nothing about what the working tree held. An uncommitted tree that gets clobbered — by a reset, by a bad merge, by the agent overwriting a file it misread — is simply gone.
- **There is no `git undo`.** The recovery rituals that do exist — `reflog`, `fsck --lost-found`, stash archaeology — assume a human who checkpointed along the way. Agents do not checkpoint.

## The net

fufu [snapshots the working tree before every action](../concepts/snapshots-and-undo.md), automatically, with no verb for asking. With the agent hooks wired — [setup](setup.md) shows how — that happens before every tool call the agent makes: every edit, every shell command, at machine rate.

Each snapshot records the tree and all refs together on one operation log. So fufu holds a running record of the agent's work that no other tool has, taken the moment before each action rather than whenever someone remembered to save.

### The net covers git itself

[`ff git <args…>`](../reference/cli/git.md) snapshots first and then runs git verbatim, and the recommended alias plus the agent hooks route the agent's git invocations through it.

A `reset --hard` still resets — fufu never blocks a command you ran, per [the two regimes](../concepts/two-regimes.md) — but the tree it discarded is now one operation back.

Even git run entirely around fufu is absorbed into the operation log at the next fufu invocation, labeled as foreign — made outside fufu, behind its back — and undoable like anything else.

The one honest gap is a foreign tree change that moves no ref, which is invisible until the next capture. Closing that gap is exactly what wiring the hooks buys, because with them the last capture is always the moment before the agent's action.

### The human keeps the last word

[`ff undo`](../reference/cli/undo.md) takes back the last operation, refs and working tree together, whether the agent did it through fufu or behind its back. A run of machine-rate captures collapses into one undo step, so unwinding an agent's session is a few keystrokes rather than an archaeology project.

## A surface with fewer ways to go wrong

Part of the argument is what fufu removes.

- **No staging area**, so there is no half-staged index for an agent to mangle, and no class of bug where the commit contains something other than the tree. The working tree is the change, and [`ff commit -m`](../reference/cli/commit.md) closes it.
- **No stash**, so there is nothing for an agent to stash and forget. Switching branches parks whatever is uncommitted with the branch you leave, and switching back resumes it.

Each ritual git demands is a place an agent can leave the repository in a state neither it nor you expected. fufu deletes the rituals rather than documenting them.

What remains is legible to a machine. Every verb reports what it did in one line, with `undo:` on the next naming the way back:

```console
$ ff start
minted ff/hidden-wren (forked from main)
open change on ff/hidden-wren
undo: ff undo
```

The agent reads its own escape hatch after every action, instead of inferring repository state from a status block built for eyes.

When it needs structure, `--json` carries each verb's full data model, errors carry stable ids, and every prompt has a non-interactive answer. The [machine surface](machine-surface.md) covers that contract in full.

## The supervisor pattern

Here is the setup this enables. The agent works — through `ff`, or even through raw git under the alias — and you review afterward, with real leverage.

- [`ff history`](../reference/cli/history.md) is the review at the coarse grain: one row per undo step, with the agent's capture noise collapsed into the rows it would undo as, so the session reads as a short list of decisions rather than hundreds of operations.
- [`ff op diff`](../reference/cli/op-diff.md) answers the finer question — what changed between any two operations, tree and refs — so you can inspect exactly what a stretch of agent work did before deciding whether it stays.
- [`ff op log 'session(<id>)'`](../reference/cli/op-log.md) isolates one agent's work even when two were interleaving, because sessions tag every operation an agent records.

Then the verdict is cheap in both directions. Work that holds up gets committed as usual. Work that does not costs one `ff undo`, and a disaster mid-session costs the same.

That changes what you are willing to let an agent attempt, because the downside of a wrong turn is no longer the afternoon.

## Strict mode as a leash

Review-after is not always enough leash, so `fufu.gitPolicy` graduates what fufu does about the agent typing git at all.

The default, `coach`, injects a one-line correction into the agent's context the first time each git word comes up — `tip: that's ff commit`. That is usually sufficient, since the agent reads it as an instruction.

`strict` refuses instead. An unambiguous git write that has a fufu verb is denied through the hook, naming the verb to run.

The limits of the leash are deliberate, and worth knowing before you rely on it:

- fufu only refuses what it can answer, so git words with no fufu equivalent — `apply`, `am`, `bisect`, `submodule` — pass untouched.
- `ff git <args…>` stays an open escape hatch even under strict for those words. A word fufu does have a verb for is refused there too, `commit -p` included.
- Ambiguous shell strings fail open, because guessing at someone else's compound command is the wrong risk to take.

Strict mode is a nudge with teeth. The actual guarantee sits underneath it: the capture already happened before the command ran, so whichever way the policy call goes, the net is intact.

## What this rests on

None of the above is special agent machinery bolted onto the side.

- The snapshot-before-every-action rule is [fufu's foundation](../concepts/snapshots-and-undo.md) for humans too. An agent is simply a writer that exercises it harder.
- The repository stays [a boring git repository](../concepts/invariant.md) throughout, so nothing the agent does through fufu can put it in a state your other tools cannot read.
- [The two regimes](../concepts/two-regimes.md) mean an agent that calls `ff` gets everything the surface promises — automation is inside the surface with everyone else.

[Setup](setup.md) is the wiring, and it takes a few minutes.
