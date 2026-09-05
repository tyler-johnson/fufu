# ff hook zsh

Three marked lines appended to the end of `$ZDOTDIR/.zshrc`, or `~/.zshrc` when `ZDOTDIR` is unset: the alias, so every git command you type runs through [`ff git`](../../reference/cli/git.md) and snapshots first, and a `precmd` function, so a snapshot lands at every prompt. The file is created if it is missing.

## What it writes

```console
$ ff hook zsh
zsh wired into ~/.zshrc
  restart the shell (or source the file) to activate it

$ cat ~/.zshrc
alias git='ff git'  # fufu — added by `ff hook`
_fufu_ambient() { ff trigger shell }  # fufu — added by `ff hook`
precmd_functions+=(_fufu_ambient)  # fufu — added by `ff hook`
```

Every line fufu writes ends in the marker `# fufu — added by \`ff hook\``, which is how fufu tells its own lines from yours. Running [`ff hook zsh`](../../reference/cli/hook.md) on a wired file reports both pieces as already wired and changes nothing.

The alias and the prompt hook are independent: a hand-written `alias git=` line naming `ff git`, or a hand-written line naming [`ff trigger shell`](../../reference/cli/trigger.md), is detected, reported as written by hand, and left alone, and the other piece is still installed.

Older markers (`ff hook shell install`, `ff shell install`) and the older prompt command `ff hook shell trigger` still count as fufu's. The next `ff hook zsh` rewrites them in place, and [`ff doctor`](../../reference/cli/doctor.md) reports them as stale until then.

## What `ff unhook zsh` removes

Exactly the marked lines. Everything else in the file stays where it was.

```console
$ ff unhook zsh
zsh removed the alias and the prompt hook from ~/.zshrc

$ cat ~/.zshrc
```

A hand-written alias or prompt hook is reported and stays.

## Notes

Restart the shell or source the file to activate it.

The prompt hook prints nothing. `ff trigger shell` captures the working copy and says nothing.

`precmd_functions+=` is additive, so the order against a prompt framework does not matter: a framework that adds its own `precmd` function before or after fufu's lines leaves the hook in place. `echo $precmd_functions` in a new shell lists `_fufu_ambient`, and `type git` shows the alias.
