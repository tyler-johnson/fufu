# ff git

Snapshots first, then runs the git command. Nothing is ever translated: the command that runs is the one you typed, or none at all. This is what the shell alias runs — `alias git='ff git'`, installed by [`ff hook <shell>`](hook.md) — so typed git keeps working exactly as it did, with a snapshot in front of it.

What `fufu.gitPolicy` decides is what fufu *says* about a git word it has a verb for:

- observe records it and stays quiet.
- coach — the default — adds one line naming the fufu verb, once per word.
- strict refuses that word outright and says what to run instead.

Words fufu has no verb for (`apply`, `bisect`, `gc`) are never touched under any tier.

The same setting governs raw git in an agent's own shell, through the hook: there coach injects the alternative into the model's context and strict asks the client to stop the call.

Every flag here belongs to git, including --help. This page is `ff help git`.

## Usage

```
Usage: ff git [OPTIONS] [ARGS]...

Arguments:
  [ARGS]...
          Arguments passed to git verbatim

Options:
      --json
          Emit machine-readable JSON

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>
```

## Examples

```
ff git status                  snapshot, then real git status
ff git commit -m "…"           git's, plus a line naming ff commit
ff config gitPolicy strict     refuse the words fufu has verbs for
ff config gitPolicy observe    count them and say nothing
ff git rebase -i HEAD~3        no fufu verb to name: snapshot, then git
ff hook zsh                    make every typed git command do this
```
