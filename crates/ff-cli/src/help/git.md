Run a Git command through fufu's snapshot and Git-policy checks. Attempts a snapshot before a permitted command; a strict-policy refusal happens before capture. If capture fails, fufu warns and Git still runs. Read this page with `ff help git`: `ff git --help` is Git's help.

## Examples

```sh
ff git status                   # Snapshot, then Git status
ff git tag                      # List tags through Git
ff config gitPolicy strict      # Refuse covered Git commands
ff config gitPolicy observe     # Record policy use without advice
ff help git                     # Fufu's passthrough help
```

### Options

### Arguments and policy

All arguments after `git` pass to Git verbatim, including flags such as `--help`. Git controls its output; `--json` does not wrap it in fufu's envelope.

`fufu.gitPolicy` governs commands for which fufu has an alternative:

- `observe` records policy use and stays quiet.
- `coach` (default) adds advice naming the fufu command, once per word.
- `strict` refuses the covered command and names the alternative. It still records the policy tally.

Commands with no covered alternative, such as apply, bisect, and gc, run in every mode. Git tag and merge also run in every mode. Git commit and rebase are refused under strict policy. Git push is classified by command, so strict also refuses tag pushes even though fufu's push sends branches only.

### Shell and agent integration

The active shell alias `alias git='ff git'`, installed by `ff hook <shell>`, routes typed Git commands here. Activate that configuration as instructed; editing an rc file alone does not change the current shell.

Agent hooks attempt capture before evaluating policy for received raw Git calls. Only Claude Code emits pre-tool coaching or denial replies; its client must enforce the denial. Codex, Qwen Code, and Cursor capture and tally recognized writes but emit no policy reply, including under strict.
