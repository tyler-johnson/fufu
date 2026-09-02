# ff hook fish

Two marked lines appended to the end of `$XDG_CONFIG_HOME/fish/config.fish`, or `~/.config/fish/config.fish` when `XDG_CONFIG_HOME` is unset: the alias, so every git command you type runs through `ff git` and snapshots first, and a `fish_prompt` event handler, so a snapshot lands at every prompt. The file and its directory are created if they are missing.

## What it writes

```console
$ ff hook fish
fish wired into ~/.config/fish/config.fish
  restart the shell (or source the file) to activate it

$ cat ~/.config/fish/config.fish
alias git 'ff git'  # fufu — added by `ff hook`
function _fufu_ambient --on-event fish_prompt; ff trigger shell; end  # fufu — added by `ff hook`
```

Every line fufu writes ends in the marker `# fufu — added by \`ff hook\``, which is how fufu tells its own lines from yours. The alias and the prompt hook are independent: a hand-written `alias git` line naming `ff git`, or a hand-written line naming `ff trigger shell`, is detected, reported as written by hand, and left alone, and the other piece is still installed. Running `ff hook fish` on a wired file reports both pieces as already wired and changes nothing.

Older markers (`ff hook shell install`, `ff shell install`) and the older prompt command `ff hook shell trigger` still count as fufu's. The next `ff hook fish` rewrites them in place, and `ff doctor` reports them as stale until then.

## What `ff unhook fish` removes

Exactly the marked lines. Everything else in the file stays where it was.

```console
$ ff unhook fish
fish removed the alias and the prompt hook from ~/.config/fish/config.fish

$ cat ~/.config/fish/config.fish
```

A hand-written alias or prompt hook is reported and stays.

## Notes

Restart the shell or source the file to activate it.

The prompt hook prints nothing. `ff trigger shell` captures the working tree and says nothing.

Event handlers are additive, so the order against a prompt framework does not matter. `functions _fufu_ambient` in a new shell shows the handler, and `type git` shows the alias.
