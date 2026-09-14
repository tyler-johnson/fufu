<a id="ff-hook-powershell"></a>
# PowerShell

[`ff hook powershell`](../cli/hook.md) installs a `git` function and quiet prompt snapshots in a console-host profile. The function runs [`ff git`](../cli/git.md); the prompt runs [`ff trigger shell`](../cli/trigger.md). See the [shared capture and ownership rules](index.md).

## Install

```powershell
ff hook powershell
```

Check the path printed by the installer:

| Platform | Profile selected |
| --- | --- |
| Windows, PowerShell 7 profile exists | `<Documents>\PowerShell\Microsoft.PowerShell_profile.ps1` |
| Windows, only the 5.1 profile exists | `<Documents>\WindowsPowerShell\Microsoft.PowerShell_profile.ps1` |
| Windows, neither profile exists | Creates the PowerShell 7 profile |
| Linux or macOS | `$XDG_CONFIG_HOME/powershell/Microsoft.PowerShell_profile.ps1`, or `~/.config/powershell/Microsoft.PowerShell_profile.ps1` |

`<Documents>` is Windows' resolved Documents folder, including OneDrive redirection. When both Windows profiles exist, the installer selects 7's.

## Activate

Restart the matching PowerShell host, or dot-source the profile if `$PROFILE` matches the installed path:

```powershell
$PROFILE
. $PROFILE
```

Other hosts can use a different profile. If the paths differ, put the two lines below in the profile your host actually loads, or explicitly dot-source the installed file.

## Verify

```powershell
Get-Command git
$function:prompt
ff hook -l
```

`git` should be a function forwarding to `ff git`, and `prompt` should run `ff trigger shell` before `_fufu_prompt`. The list command checks installed files. To test prompt capture, edit a disposable file, return to the prompt, then inspect [`ff history`](../cli/history.md) for the shell capture.

<a id="what-it-writes"></a>
## Files changed

```console
$ ff hook powershell
powershell wired into ~/.config/powershell/Microsoft.PowerShell_profile.ps1
  restart the shell (or source the file) to activate it

$ cat ~/.config/powershell/Microsoft.PowerShell_profile.ps1
function git { ff git @args }  # fufu — added by `ff hook`
if (-not (Test-Path Function:_fufu_prompt)) { $function:global:_fufu_prompt = $function:prompt; function global:prompt { ff trigger shell | Out-Null; _fufu_prompt } }  # fufu — added by `ff hook`
```

The file and parent directory are created if missing. `@args` forwards the Git arguments. The prompt wrapper saves the previous prompt, captures, and calls it; its guard prevents wrapping twice, and `Out-Null` keeps trigger output out of the prompt text. CRLF line endings are preserved.

The marker identifies managed lines; recognized hand-written function and prompt lines survive as described in the [overview](index.md#files-changed).

<a id="what-ff-unhook-powershell-removes"></a>
## Remove

[`ff unhook powershell`](../cli/unhook.md) removes the marked lines. Restart PowerShell afterward to unload the function and wrapper.

```console
$ ff unhook powershell
powershell removed the alias and the prompt hook from ~/.config/powershell/Microsoft.PowerShell_profile.ps1

$ cat ~/.config/powershell/Microsoft.PowerShell_profile.ps1
```

A hand-written function or prompt hook is reported and stays.

<a id="notes"></a>
## Troubleshooting and migration

- **Prompt frameworks:** initialize them above fufu's lines. A later replacement of `prompt` removes the wrapper. After changing the order, start a new shell so `_fufu_prompt` saves the intended prompt.
- **Other Git functions:** the last definition wins. An unrelated hand-written `git` function stays in the file, but fufu's appended function takes precedence. Removing fufu's lines and restarting restores the earlier definition.
- **Older installations:** `ff hook powershell` upgrades the retired markers (`ff hook shell install`, `ff shell install`) and `ff hook shell trigger`. [`ff doctor`](../cli/doctor.md) reports them as stale until repaired.
