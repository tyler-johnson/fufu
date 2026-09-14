<a id="ff-hook-bash"></a>
# Bash

[`ff hook bash`](../cli/hook.md) installs a `git` alias and quiet prompt snapshots in `~/.bashrc`. The alias runs [`ff git`](../cli/git.md); the prompt runs [`ff trigger shell`](../cli/trigger.md). See the [shared capture and ownership rules](index.md).

## Install

```sh
ff hook bash
```

## Activate

Open a new Bash shell, or load the file in the current one:

```sh
source ~/.bashrc
```

## Verify

```sh
type git
printf '%s\n' "$PROMPT_COMMAND"
ff hook -l
```

`type git` should show `git='ff git'`, and `PROMPT_COMMAND` should begin with `ff trigger shell`. The list command checks the file, not the running shell. To test prompt capture, edit a disposable file, return to the prompt, then inspect [`ff history`](../cli/history.md) for the shell capture.

<a id="what-it-writes"></a>
## Files changed

```console
$ ff hook bash
bash wired into ~/.bashrc
  restart the shell (or source the file) to activate it

$ cat ~/.bashrc
alias git='ff git'  # fufu — added by `ff hook`
[[ $PROMPT_COMMAND == *"ff trigger shell"* ]] || PROMPT_COMMAND="ff trigger shell;$PROMPT_COMMAND"  # fufu — added by `ff hook`
```

The file is created if missing. The marker identifies managed lines; recognized hand-written alias and prompt lines survive as described in the [overview](index.md#files-changed).

<a id="what-ff-unhook-bash-removes"></a>
## Remove

[`ff unhook bash`](../cli/unhook.md) removes the marked lines. Restart Bash afterward to unload the alias and prompt hook.

```console
$ ff unhook bash
bash removed the alias and the prompt hook from ~/.bashrc

$ cat ~/.bashrc
```

A hand-written alias or prompt hook is reported and stays. Unhook only takes back what hook added.

<a id="notes"></a>
## Troubleshooting and migration

Keep prompt-framework initialization above fufu's lines. A later assignment to `PROMPT_COMMAND` replaces the hook; the installer appends at the end, so a framework already configured is normally in the right order.

Older markers (`ff hook shell install`, `ff shell install`) and `ff hook shell trigger` are upgraded by `ff hook bash`. [`ff doctor`](../cli/doctor.md) reports these as stale until repaired.
