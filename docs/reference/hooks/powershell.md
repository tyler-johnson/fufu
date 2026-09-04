# ff hook powershell

Two marked lines appended to the end of PowerShell's profile, the file `$PROFILE` names in the console host: a `git` function, so every git command you type runs through [`ff git`](../../reference/cli/git.md) and snapshots first, and a wrapped `prompt`, so a snapshot lands at every prompt. The file and its directory are created if they are missing.

Which file that is depends on the platform:

- On Windows, `<Documents>\PowerShell\Microsoft.PowerShell_profile.ps1` for PowerShell 7 and `<Documents>\WindowsPowerShell\Microsoft.PowerShell_profile.ps1` for Windows PowerShell 5.1, where `<Documents>` is the Documents folder as Windows resolves it, so a OneDrive-redirected Documents lands on the file PowerShell reads. The slug wires the 7 file when it exists or when neither exists, and the 5.1 file only when it is the sole profile on disk.
- On Linux and macOS, `$XDG_CONFIG_HOME/powershell/Microsoft.PowerShell_profile.ps1`, or `~/.config/powershell/Microsoft.PowerShell_profile.ps1` when `XDG_CONFIG_HOME` is unset, which is the path the blocks below show.

## What it writes

```console
$ ff hook powershell
powershell wired into ~/.config/powershell/Microsoft.PowerShell_profile.ps1
  restart the shell (or source the file) to activate it

$ cat ~/.config/powershell/Microsoft.PowerShell_profile.ps1
function git { ff git @args }  # fufu — added by `ff hook`
if (-not (Test-Path Function:_fufu_prompt)) { $function:global:_fufu_prompt = $function:prompt; function global:prompt { ff trigger shell | Out-Null; _fufu_prompt } }  # fufu — added by `ff hook`
```

The alias is a function because `Set-Alias` takes no arguments, and `@args` forwards whatever you typed after `git`. The prompt line saves the current `prompt` under `_fufu_prompt` and redefines `prompt` to run the trigger and then call it; the `Test-Path` guard means a profile dot-sourced twice wraps once, and `Out-Null` keeps anything the trigger ever writes out of the prompt string, since the prompt function's output is the prompt.

Every line fufu writes ends in the marker `# fufu — added by \`ff hook\``, which is how fufu tells its own lines from yours. Running [`ff hook powershell`](../../reference/cli/hook.md) on a wired file reports both pieces as already wired and changes nothing.

The function and the prompt hook are independent: a hand-written `function git` line naming `ff git`, or a hand-written line naming [`ff trigger shell`](../../reference/cli/trigger.md), is detected, reported as written by hand, and left alone, and the other piece is still installed.

Older markers (`ff hook shell install`, `ff shell install`) and the older prompt command `ff hook shell trigger` still count as fufu's. The next `ff hook powershell` rewrites them in place, and [`ff doctor`](../../reference/cli/doctor.md) reports them as stale until then. A profile with CRLF line endings keeps them through every rewrite.

## What `ff unhook powershell` removes

Exactly the marked lines. Everything else in the file stays where it was.

```console
$ ff unhook powershell
powershell removed the alias and the prompt hook from ~/.config/powershell/Microsoft.PowerShell_profile.ps1

$ cat ~/.config/powershell/Microsoft.PowerShell_profile.ps1
```

A hand-written function or prompt hook is reported and stays.

## Notes

Restart PowerShell or dot-source the profile (`. $PROFILE`) to activate it. A file is not a running shell.

The prompt hook prints nothing. `ff trigger shell` captures the working tree and says nothing, so a prompt with fufu wired looks like a prompt without it.

The prompt line wraps whatever `prompt` is when the profile reaches it. A prompt framework initialized below fufu's lines that redefines `prompt` outright drops the hook. fufu appends at the end of the file, so a framework already in the file is already above the marked lines. If you add one later, keep its init line above them. Check in a new shell:

```powershell
$function:prompt
```

It should begin with `ff trigger shell`. `Get-Command git` in the same shell shows `Function`.

A later `function git` in the file wins over an earlier one. A hand-written `git` function that forwards to something other than `ff git` is not claimed by fufu, so `ff hook powershell` adds its own below it and the shell uses fufu's; [`ff unhook powershell`](../../reference/cli/unhook.md) removes only fufu's line and yours is in effect again.

PowerShell 7 and Windows PowerShell 5.1 read different files, and the slug wires 7's unless only 5.1's exists. If your `$PROFILE` names some other file, a host other than the console or a profile you moved, that file is not this one, and the two lines above pasted into it work the same way.
