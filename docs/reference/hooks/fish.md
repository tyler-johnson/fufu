<a id="ff-hook-fish"></a>
# Fish

[`ff hook fish`](../cli/hook.md) installs a `git` alias and quiet prompt snapshots in `$XDG_CONFIG_HOME/fish/config.fish`, or `~/.config/fish/config.fish` when `XDG_CONFIG_HOME` is unset. The alias runs [`ff git`](../cli/git.md); the prompt runs [`ff trigger shell`](../cli/trigger.md). See the [shared capture and ownership rules](index.md).

## Install

```fish
ff hook fish
```

## Activate

Open a new Fish shell, or source the file named by the installer:

```fish
if set -q XDG_CONFIG_HOME
    source "$XDG_CONFIG_HOME/fish/config.fish"
else
    source ~/.config/fish/config.fish
end
```

## Verify

```fish
type git
functions _fufu_ambient
ff hook -l
```

The alias should run `ff git`, and `_fufu_ambient` should handle `fish_prompt` by running `ff trigger shell`. The list command checks installed files. To test prompt capture, edit a disposable file, return to the prompt, then inspect [`ff history`](../cli/history.md) for the shell capture.

<a id="what-it-writes"></a>
## Files changed

```console
$ ff hook fish
fish wired into ~/.config/fish/config.fish
  restart the shell (or source the file) to activate it

$ cat ~/.config/fish/config.fish
alias git 'ff git'  # fufu — added by `ff hook`
function _fufu_ambient --on-event fish_prompt; ff trigger shell; end  # fufu — added by `ff hook`
```

The file and parent directory are created if missing. The marker identifies managed lines; recognized hand-written alias and prompt lines survive as described in the [overview](index.md#files-changed).

<a id="what-ff-unhook-fish-removes"></a>
## Remove

[`ff unhook fish`](../cli/unhook.md) removes the marked lines. Restart Fish afterward to unload the alias and event handler.

```console
$ ff unhook fish
fish removed the alias and the prompt hook from ~/.config/fish/config.fish

$ cat ~/.config/fish/config.fish
```

A hand-written alias or prompt hook is reported and stays.

<a id="notes"></a>
## Troubleshooting and migration

The event handler works alongside prompt frameworks without wrapping their prompt function. Check that `_fufu_ambient` remains defined if a later configuration removes functions.

Older markers (`ff hook shell install`, `ff shell install`) and `ff hook shell trigger` are upgraded by `ff hook fish`. [`ff doctor`](../cli/doctor.md) reports these as stale until repaired.
