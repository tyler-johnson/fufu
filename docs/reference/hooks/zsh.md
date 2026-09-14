<a id="ff-hook-zsh"></a>
# Zsh

[`ff hook zsh`](../cli/hook.md) installs a `git` alias and quiet prompt snapshots in `$ZDOTDIR/.zshrc`, or `~/.zshrc` when `ZDOTDIR` is unset. The alias runs [`ff git`](../cli/git.md); the prompt runs [`ff trigger shell`](../cli/trigger.md). See the [shared capture and ownership rules](index.md).

## Install

```sh
ff hook zsh
```

## Activate

Open a new Zsh shell, or load its configuration:

```sh
source "${ZDOTDIR-$HOME}/.zshrc"
```

## Verify

```sh
type git
print -l $precmd_functions
ff hook -l
```

The alias should run `ff git`, and the function list should contain `_fufu_ambient`. The list command checks installed files. To test prompt capture, edit a disposable file, return to the prompt, then inspect [`ff history`](../cli/history.md) for the shell capture.

<a id="what-it-writes"></a>
## Files changed

```console
$ ff hook zsh
zsh wired into ~/.zshrc
  restart the shell (or source the file) to activate it

$ cat ~/.zshrc
alias git='ff git'  # fufu — added by `ff hook`
_fufu_ambient() { ff trigger shell }  # fufu — added by `ff hook`
precmd_functions+=(_fufu_ambient)  # fufu — added by `ff hook`
```

The file is created if missing. The marker identifies managed lines; recognized hand-written alias and prompt lines survive as described in the [overview](index.md#files-changed).

<a id="what-ff-unhook-zsh-removes"></a>
## Remove

[`ff unhook zsh`](../cli/unhook.md) removes the marked lines. Restart Zsh afterward to unload the alias and prompt function.

```console
$ ff unhook zsh
zsh removed the alias and the prompt hook from ~/.zshrc

$ cat ~/.zshrc
```

A hand-written alias or prompt hook is reported and stays.

<a id="notes"></a>
## Troubleshooting and migration

`precmd_functions+=` adds fufu's function without replacing other prompt callbacks. A framework that also appends works in either order; one that replaces the entire list can remove the hook. Repeated sourcing appends the function again, so use a new shell when checking a clean configuration.

Older markers (`ff hook shell install`, `ff shell install`) and `ff hook shell trigger` are upgraded by `ff hook zsh`. [`ff doctor`](../cli/doctor.md) reports these as stale until repaired.
