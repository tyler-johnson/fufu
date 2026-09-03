# ff hook bash

Two marked lines appended to the end of `~/.bashrc`: the alias, so every git command you type runs through [`ff git`](../../reference/cli/git.md) and snapshots first, and the prompt hook, so a snapshot lands at every prompt. The file is created if it is missing.

## What it writes

```console
$ ff hook bash
bash wired into ~/.bashrc
  restart the shell (or source the file) to activate it

$ cat ~/.bashrc
alias git='ff git'  # fufu — added by `ff hook`
[[ $PROMPT_COMMAND == *"ff trigger shell"* ]] || PROMPT_COMMAND="ff trigger shell;$PROMPT_COMMAND"  # fufu — added by `ff hook`
```

Every line fufu writes ends in the marker `# fufu — added by \`ff hook\``, and the marker is how fufu tells its own lines from yours. The two pieces are independent. A hand-written `alias git=` line naming `ff git`, or a hand-written line naming [`ff trigger shell`](../../reference/cli/trigger.md), is detected, reported as written by hand, and left alone, and the other piece is still installed. Running [`ff hook bash`](../../reference/cli/hook.md) on a wired file reports both pieces as already wired and changes nothing.

Older installs carry the markers `# fufu — added by \`ff hook shell install\`` and `# fufu — added by \`ff shell install\``, and an older prompt line calls `ff hook shell trigger`. All of them still count as fufu's. The next `ff hook bash` rewrites them in place to the current spelling, and [`ff doctor`](../../reference/cli/doctor.md) reports them as stale until then.

## What `ff unhook bash` removes

Exactly the marked lines. Everything else in the file stays where it was.

```console
$ ff unhook bash
bash removed the alias and the prompt hook from ~/.bashrc

$ cat ~/.bashrc
```

A hand-written alias or prompt hook is reported and stays. Unhook only takes back what hook added.

## Notes

Restart the shell or source the file to activate it. A file is not a running shell.

The prompt hook prints nothing. `ff trigger shell` captures the working tree and says nothing, so a prompt with fufu wired looks like a prompt without it.

The bash line prepends to whatever `PROMPT_COMMAND` holds when the rc file reaches it. A prompt framework initialized below fufu's lines that assigns `PROMPT_COMMAND` outright drops the hook. fufu appends at the end of the file, so a framework already in the file is already above the marked lines. If you add one later, keep its init line above them. Check in a new shell:

```sh
echo "$PROMPT_COMMAND"
```

It should begin with `ff trigger shell`. `type git` in the same shell shows the alias.
