# Doctor

Run [`ff doctor`](cli/doctor.md) to check operation history, configuration, and installed integrations. Use it after setup or an upgrade, or when recovery, hooks, or remote selection behave unexpectedly.

```sh
ff doctor --no-fetch            # Check using existing tracking refs
ff doctor --fix --no-fetch      # Repair supported findings
ff doctor --json --no-fetch     # Read diagnostic rows in a script
```

## Verdicts

| Row | Meaning | Action |
| --- | --- | --- |
| `ok` | This particular check passed | No repair indicated |
| `info` | Informational state, such as a setting or retention preview | Read if relevant to your task |
| `WARN` | A finding | Follow the row's advice or use a supported repair |

A completed report exits **0** with no findings and **1** with findings. JSON contains `checks`, `findings`, and `fixable`. A command error can also exit nonzero; scripts should inspect the [JSON envelope](../agents/machine-surface.md#the-envelope).

An `ok` hook row means configuration was found. It does not prove a running shell loaded it, a client activated it, or Codex approved it. Follow the [hook activation steps](hooks/index.md#activate) and the [agent capture-and-recovery check](../agents/setup.md#verify).

<a id="the-one-write-fix"></a>
## Repairs: --fix

`--fix` performs these repairs:

| Finding | Repair |
| --- | --- |
| `gc config` | Set local `gc.refs/fufu/*.reflogExpire` and `reflogExpireUnreachable` to `never` |
| `upstreams` | For an absent local branch, remove its config section if its tracking ref is absent or cannot be named |
| Partial or stale managed hooks | Re-run that integration's repair, including retired shell spellings |
| Stale shipped skill | Refresh it through the client's installer |

A surviving tracking ref keeps its branch configuration. With `--no-fetch`, that decision uses cached tracking refs, not a fresh query to the server. Repairs do not install every absent client or approve Codex hooks. Restart or reload the affected shell/client after hook repairs, and review changed Codex hooks through `/hooks`.

Re-run doctor after repairs. An invalid setting, signing problem, moved operation ref, or missing reflog needs the specific action described below; `--fix` does not repair those automatically.

## Capture, fetch, and maintenance

Doctor is not read-only at the CLI boundary, even without `--fix`:

- Before the checks, it attempts a snapshot. This can initialize the operation log, write recovery objects and configuration, or reconcile outside ref changes.
- It fetches every run when automatic fetching is enabled. `--no-fetch` skips it; CI skips it unless `--fetch` is explicit.
- Update checks can start before the report. Automatic trimming and update maintenance can run afterward, including after a report with findings. `--no-fetch` does not disable these.

See [configuration](config.md) for `autoFetch`, `autoTrim`, and `updateCheck`. A row describes the state at check time; maintenance can change that state afterward.

<a id="a-healthy-run-annotated"></a>
## Sample report

These selected rows illustrate a repository with a Bash hook installed; paths and ages depend on the checkout:

```text
  ok    identity       the log tip is a fufu operation
  ok    gc config      reflog expiry disabled for refs/fufu/*
  info  trim           nothing to drop — every operation is inside the keep window
  info  signing        off (commit.gpgsign)
  ok    alias          git='ff git' wired in ~/.bashrc (`ff hook bash` manages it)
```

The summary counts warnings and supported repairs, for example:

```text
2 finding(s) — `ff doctor --fix` repairs 1 of them
```

## Check catalog

<a id="the-engine"></a>
### Repository and operation history

| Check | What it reports |
| --- | --- |
| `repository` | Git directory; outside a repository or in a bare repository, repository checks are skipped |
| `log` | Current worktree's operation-log ref and newest age; warns when absent after the capture attempt |
| `identity` | Whether the log tip has the structure of a fufu operation; warns otherwise |
| `pointers` | Stored branch pointers and their ages; this lists pointers rather than validating that every target belongs to the current log |
| `reflogs` | Whether the operation ref has a reflog; warns when absent |
| `gc config` | Whether both local fufu reflog-expiry keys are `never`; repairable warning otherwise |
| `trash` | Retained pre-trim tips, when present |
| `objects` | Loose-object and pack counts; an informational suggestion to run [`ff op trim`](cli/op-trim.md) when loose objects reach `gc.auto` |
| `id index` | Whether the short-operation-ID index is present and current; absent/stale is informational and a later indexed read rebuilds it |
| `last op` | Newest operation's summary and age; warns if it cannot parse the tip |
| `drift` | Outside ref changes still pending after the preflight capture/reconciliation attempt |
| `legacy` | Pre-cutover refs under `refs/fufu/legacy/` that this version cannot read |
| `parked` | [Parked changes](../concepts/changes.md#parking-and-resuming) saved for other branches; a separate row can name old stash-based parks awaiting migration on switch |

The reflog supports [`ff redo`](cli/redo.md), past-time queries, and access to abandoned operation history. Missing entries cannot be recreated from the row. The `id index` check itself only reads status; the CLI's separate preflight and maintenance effects still apply.

Manual `ff op trim` invokes Git's `gc --auto`; automatic trim does not. The `objects` row counts storage without packing it.

### Settings, retention, and signing

| Check | What it reports |
| --- | --- |
| `settings` | Registry-known values; warns on invalid values, lists valid overrides as information |
| `trim` | Preview of operations outside `keep`; informational, omitted when `keep` is invalid |
| `auto-trim` | Whether automatic trimming is enabled, its cadence, and last-run time |
| `auto-fetch` | Fetch setting and recorded fetch state; warns on recorded failures when enabled |
| `signing` | Signing enabled/disabled, recognized format, program lookup, and required SSH configuration |

The signing check does not invoke a signer, run `gpg.ssh.defaultKeyCommand`, test key access, or verify a signature. For SSH it checks that an allowed-signers path is configured, not that the file exists or trusts the key. Complex program strings can defer validation until execution. Use the [signing workflow](signing.md) to test creation and verification separately.

<a id="the-remote-floor"></a>
### Remotes and branches

| Check | What it reports |
| --- | --- |
| `remotes` | Whether local branches can select a configured remote; ambiguous selection is a warning; local-only repositories omit this row |
| `upstreams` | Configuration for absent local branches: informational while a tracking ref survives, repairable when none does |
| `tracking` | Existing local branches whose configured tracking ref is absent; informational |

For ambiguous selection, choose a remote with [`ff push --to <remote>`](cli/push.md) when ready to send the branch. [`ff pull`](cli/pull.md) reports fetch failures in more detail. The [network configuration advice](config.md#what-fufu-reads-from-gits-config) covers credentials and native HTTP proxy limits.

### Raw git

`raw git` summarizes recognized Git writes and policy decisions counted in this worktree. It is absent when nothing has been counted. These are observations and requested denials, not proof a client prevented a command: Codex, Qwen Code, OpenCode, and Cursor do not emit policy replies. See [Git policy](../agents/setup.md#pick-a-git-policy).

<a id="the-wiring"></a>
### Installed hooks and skills

These checks use the same installed-file status as [`ff hook -l`](cli/hook.md). The [hook reference](hooks/index.md) lists files and activation commands.

- **Client names** (`claude`, `codex`, `qwen`, `opencode`, `cursor`): installed managed configuration is `ok`; partial or stale managed configuration is a repairable warning. A detected but unconfigured client is optional information. A client neither detected nor installed has no row.
- **Shell names:** retired managed shell spellings produce a repairable warning under the shell's name.
- **`alias` and `ambient`:** alias/function and prompt hook are reported separately, across all supported shell files. One installed shell can satisfy each row. Hand-written matches are heuristic information; check the actual shell.
- **`skill`:** shipped skills are aggregated across clients. Absence is optional information; stale content is a repairable warning named for the affected client.
- **`triggers`:** warns when no recognized hook, alias, or prompt configuration is found. Its absence is not an activation test: even a partial or hand-written installation can suppress this warning.

### Extensions

`extensions` lists discovered executable `ff-<name>` commands on PATH as information. This machine-level check also runs outside a repository; it does not execute extensions or validate their behavior.

<a id="the-update-lane"></a>
### Updates

`update` reports cached update status: no check yet, up to date, an available release, or a source build. [`ff update`](cli/update.md) follows the installation channel; this row is not an independent check of the running binary's installation.

## Common failures

### The engine has never run here

A missing-log warning can remain if preflight capture failed, for example because it could not acquire a lock. Normally even the first doctor invocation attempts initialization. Check capture warnings, then retry a repository command. [Adoption](../adopting.md) explains initial setup.

### The gc guard is gone

Run `ff doctor --fix --no-fetch` to restore both expiry keys to `never`. This protects retained reflog entries from ordinary Git expiry; it does not restore entries already lost.

### The log ref was moved by something other than fufu

An `identity` or `last op` warning can mean the operation ref points at a non-operation object. `--fix` does not repair it. Preserve the repository and inspect the named ref's reflog to identify the last valid operation before attempting a manual repair. A blind `@{1}` reset can select another invalid entry and leave branch pointers inconsistent; report the finding if the correct state is uncertain.

### The log ref has no reflog

A later fufu operation that updates the ref recreates its reflog, and new entries accumulate from there. The missing entries remain lost: redo and time queries cannot reach states recorded only in those entries. See [recovery scope](../concepts/snapshots-and-undo.md#coverage-and-limits).

### Config naming a branch that is gone from both sides

Use `ff doctor --fix` after refreshing tracking refs, or `--fix --no-fetch` to use the current cached view. It removes the absent branch's config section only when no corresponding tracking ref survives. A surviving remote-tracking ref keeps the section.

### The wiring drifted

Run `ff hook <client>` for the named integration, or `ff doctor --fix`. Restart or reload afterward, and review changed Codex hooks through `/hooks`. A hand-authored command not recognized as managed must be corrected by its owner.

## When to run it

Run doctor after adoption, after updating fufu, or when a specific operation or hook appears broken. In CI, interpret the report and exit status together; machine-level hook findings may reflect the CI environment rather than the repository.
