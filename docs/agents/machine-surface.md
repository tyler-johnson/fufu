<a id="the-machine-surface"></a>
# JSON output and scripting

Reporting commands accept `--json` and write one versioned JSON envelope on stdout. Read the payload **together with the process exit code**: `data` can describe a partial success or a held outcome even when the exit is nonzero. Use a pinned, tested fufu binary and parse fields by name.

## The envelope

An envelope has `ff` (currently `1`), `cmd` (the responding command), and either `data` or `error`, never both. Here is a small Python example using [`ff status`](../reference/cli/status.md) in the current directory:

```python
import json
import os
import subprocess

result = subprocess.run(
    ["ff", "status", "--json"],
    env={**os.environ, "FF_NONINTERACTIVE": "1"},
    capture_output=True,
    text=True,
)
report = json.loads(result.stdout)
if report["ff"] != 1 or report["cmd"] != "status":
    raise RuntimeError("unsupported fufu report")
if ("data" in report) == ("error" in report):
    raise RuntimeError("expected exactly one payload")
if "error" in report:
    error = report["error"]
    raise SystemExit(f"ff exited {result.returncode}: {error['id']}: {error['message']}")
if result.returncode != 0:
    raise SystemExit(f"ff exited {result.returncode}; inspect data: {report['data']}")
print([change["path"] for change in report["data"]["changes"]])
```

In a repository this prints the changed paths. Outside one, it reports `repo/not-found` and exits unsuccessfully. For a rewrite or push, inspect the command-specific held, skipped, and per-branch results before treating exit 0 as complete success.

`error` carries `id`, `message`, and `exits` (suggested next commands). Branch on the ID rather than message prose. [`ff explain <id>`](../reference/cli/explain.md) explains a refusal, and the [error reference](../reference/errors.md) lists the catalog. Failures before a report can be produced, such as a missing executable or an early argument-parser error, also need normal process-error handling.

## Projecting with `--fields`

`--fields <list>` keeps only the named parts of `data`: comma-separated dotted paths, applied inside the envelope. A path walks objects by key and maps over arrays, so `commits.subject` keeps `subject` on every row of `commits` and drops the rest. Kept keys keep their nesting, a key named whole keeps everything under it, and the envelope's `ff`, `cmd`, and `error` are never touched. There is no template language, renaming, or computed value; the envelope is the shape, and this trims it.

```console
$ ff log -n 1 --json --fields commits.subject,commits.body
{"ff":1,"cmd":"log","data":{"commits":[{"subject":"parser: skeleton","body":""}]}}
```

A path that matches nothing is refused with `usage/no-such-field` at exit 2, naming the deepest key it reached and the keys available there, so a typo cannot yield an empty object. The check runs against the payload actually produced: a key a view flag dropped, such as `changes` under `ff show --no-patch`, counts as missing. It also runs when the report is written, after the command's work, so on a command that changes the repository the work stands and only the report was refused. `--fields` without `--json` is refused with `usage/bad-flags`, and where `--json` is ignored, such as Git passthrough, `--fields` is ignored with it.

<a id="what-is-promised"></a>
### Compatibility in current releases

`ff: 1` identifies the current envelope format; it does not establish cross-release payload or error-ID stability. With that same version, v0.13.0 renamed sync/publish errors, v0.14.0 removed `id_letters`, and v0.15.0 renamed arrival fields and removed operation `route`.

Pin and test supported binary versions, assert the envelope version, and tolerate unknown fields. Review the [changelog](../changelog.md) when upgrading. There is no stronger established payload-versioning promise to infer from the number. Human output is for people and can change layout, color, and wording.

## Exit codes

| Code | Interpretation |
| --- | --- |
| 0 | Command completed; still inspect per-branch and cascade outcomes. |
| 1 | Failure or a negative check, including doctor findings and a refused branch in a multi-branch push. |
| 2 | Usage error, including strict Git-policy refusal. |
| 3 | Held or blocked outcome, or a dry run predicts a hold. Earlier successful updates can stand. |
| 4 | `ref/contended`: another writer held a required lock. Use a bounded retry policy. |

For error envelopes, `usage/*` maps to 2, `held/*` to 3, `ref/contended` to 4, and other errors to 1. A `data` envelope can also carry a nonzero process exit: [`ff doctor`](../reference/cli/doctor.md) reports findings at 1, and rewrite and push reports have the outcomes below.

| Command | Held and partial-success behavior |
| --- | --- |
| [`ff pull`](../reference/cli/pull.md), [`ff restack`](../reference/cli/restack.md) | Exit 3 for held replays. Pull also exits 3 for predicted holds in its dry run; restack has no dry-run flag. Successful updates elsewhere stand. |
| [`ff absorb`](../reference/cli/absorb.md), [`ff lift`](../reference/cli/lift.md), [`ff done`](../reference/cli/done.md) | Primary held replay exits 3. A successful primary landing with downstream cascade holds currently exits 0. |
| [`ff describe`](../reference/cli/describe.md) | Reword can exit 0 with nonempty `reword.cascade.held`. |
| [`ff fold`](../reference/cli/fold.md) | A primary conflict refuses at 1 without a fold hold; downstream cascade holds exit 3 after the fold lands. |
| [`ff switch`](../reference/cli/switch.md) | A parked-arrival hold exits 3 after completing the branch switch; resolve handles that arrival in place. |
| [`ff push`](../reference/cli/push.md) | Existing holds block the affected branches. Holds alone exit 3; any refused send makes the run exit 1, even alongside successful sends or held branches. |

A hold is a requested replay waiting on conflicting changes. Stop and surface it rather than retrying blindly. Cascades also name skipped branches: checked out elsewhere, already held, or containing merges. Their descendants are left alone. See [cascade recovery](../guides/rewriting-history.md#conflicts-and-dependent-branches).

Neither a refusal nor contention promises an untouched repository. Pre-capture, reconciliation, fetches, metadata, or earlier branch updates can already have happened. Inspect the result before a retry. A failed capture in a client hook can be skipped with exit 0; that is a different contract from a contended mutating command.

## Commands with different streams

The global `--json` flag does not turn every command into a single-report API:

| Command | Output and exit contract |
| --- | --- |
| [`ff git`](../reference/cli/git.md) | Permitted passthroughs use Git's stdout, stderr, and exit status. Fufu's runtime policy refusals are also human output: this command disables JSON reporting. Arguments after `git` belong to Git. |
| [`ff update`](../reference/cli/update.md) | Prints update instructions or installer output; `--check` is quiet. It disables JSON reporting, including runtime errors; read its exit status and human diagnostics. |
| [`ff trigger <client>`](../reference/cli/trigger.md) | Client-specific briefing/advice/denial protocol. Runtime failures exit 0 quietly unless `FF_DEBUG` is set. Manual trigger uses the ordinary report contract. |
| [`ff watch`](../reference/cli/watch.md) | Always newline-delimited JSON event envelopes, regardless of `--json`. Read each line's `data.motion`; the process can emit many lines before exiting. |
| Extensions | The child owns arguments, output, and exit status; see below. |

Client denials depend on the adapter and the client; [setup](setup.md#pick-a-git-policy) names the current differences. A strict `ff git` refusal happens before capture or Git execution, but records the policy tally. Agent triggers attempt capture before evaluating policy.

## Noninteractive operation

Set `FF_NONINTERACTIVE=1` to disable fufu's prompts and editors even with a terminal attached. Nonterminal stdin also disables them. Supply answers through flags, such as `-m` for a commit message; missing required answers produce a refusal. An updater may print instructions without performing an update. Git passthrough, extensions, commit hooks, credential helpers, and other external tools retain their own interaction rules.

For scripted [`ff hook`](../reference/cli/hook.md) installation, name the client rather than invoking the interactive selector. Close stdin for Claude Code installation (`ff hook claude < /dev/null`): its legacy spelling checks piped input for a trigger payload. Activation and trust remain client-specific.

### Piped output never pages

The log family pages human output only on a terminal. Piped output and JSON never page. Pager selection is `fufu.pager`, then `FF_PAGER`, then `PAGER`, then `less`. Piped human output is uncolored; `NO_COLOR` is honored. Scripts should still parse JSON rather than human rows.

## Sessions: tagging work, and asking about it

A session is a label attached to recorded operations, not an editing-session branch or a separate undo chain. There is no session-open or session-close command. For ordinary commands, tag precedence is `--session <name>`, then `FF_SESSION`, then `CLAUDE_CODE_SESSION_ID`. A valid session ID in a client hook payload takes precedence for that capture, falling back to the invocation's tag when absent or invalid.

For example, after editing a file, label a manual capture and query its records:

```sh
ff --session flight-3 trigger -m checkpoint
ff op log 'session(flight-3)' --json
ff op log 'session(flight-3) & kind(op)' --json
```

[`ff op log`](../reference/cli/op-log.md) uses `session()` to **filter existing records**. Its global `--session` flag tags any new capture made by the read; it does not filter the result. `ff watch --session flight-3` is a command-specific **stream filter**. Environment tags do not implicitly become watch filters. Tags distinguish retained operations from interleaved writers, but cannot selectively separate their file edits or preserve records beyond retention.

## Polling and streaming

Poll `ff op log --json` for retained operation records, newest first. Repository readers can capture and reconcile before returning; watch itself does neither. For a foreground stream, run `ff watch`; `-n 1` prints only its opening event and exits:

<!-- transcript:watch -->
```console
$ ff watch -n 1
{"ff":1,"cmd":"watch","data":{"worktree":"main","motion":"start","tip":"386753f42f6cb296325bfa1109c3cc35e08521f7"}}
```
<!-- /transcript -->

`start` names the initial tip; later motions are `landed`, `stepped-back`, `forked`, and `rewritten`. `--since <op>` replays from a retained operation before following new events. `--kind` and `--session` filter operation events. `--all` watches all worktrees and cannot be combined with `--since`; each event identifies its worktree.

On a rewritten chain, single-worktree watch emits `rewritten` and exits 1 because its anchor is invalidated. Reconnect and refresh affected IDs. Under `--all`, the affected chain re-anchors and the stream continues. See the [watch reference](../reference/cli/watch.md) for event details.

Operations are write-ahead records: observing one does not prove that its described ref or file mutation finished. This applies to polling and streaming alike. Check command completion and current state when that distinction matters.

## Extensions

An unknown `ff <name>` runs `ff-<name>` from PATH when available; built-in commands win. Fufu attempts capture first, then runs the child with its remaining arguments and inherited environment. It sets `FF_REPO` when a worktree root is found, `FF_CONTRACT` to the current envelope version, and `FF_SESSION` when tagged. It does not clear inherited `FF_REPO` or `FF_SESSION` when no replacement is found, so launchers should avoid exporting stale values. `--json` after the extension name belongs to that child.

Fufu does not wrap the child's output or reinterpret its status. `ff help <name>` does not dispatch to it; invoke the extension's own help. Doctor lists discovered extensions. Extensions should call fufu verbs to change fufu state rather than writing its refs directly.

## Detecting fufu programmatically

[`ff version --json`](../reference/cli/version.md) works outside repositories. Its `data` includes `version`, `commit`, `date`, and cached update status. Use it to identify the supported binary before making repository calls.

Inside a directory, `ff status --json` reports `repo/not-found` at exit 1 if no repository exists. In an existing Git repository, readers attempt capture and initialize an operation history from observed state when needed. That earliest recovery point cannot supply older uncaptured work. `ff doctor --json` checks configuration and repository findings, but an installed hook row does not prove activation or Codex trust.

## Report fields and examples

The examples below are **valid JSON projections**: by `--fields`, which shows the whole envelope, indented by `jq .`, or by the displayed `jq` filter, which shows `data` alone. `scripts/docs/machine-surface-transcript.sh` creates their scratch repository, commits a parser skeleton, edits it, and takes two captures tagged `flight-3`. IDs, times, and paths vary. Remove the filter to inspect the complete report; no omitted fields are represented by an invalid `[...]` placeholder.

Commit SHAs and operation IDs are hexadecimal; `change_id` uses k–z for identity across surviving rewrites. The argument position determines which address space an ID belongs to. `time` fields in these history reports are Unix seconds; other reports can use other timestamp names. [Revisions and IDs](../reference/revisions.md) owns prefix rules, expressions, and past-state reads.

## `ff status --json`

Status reports working-copy state, including the current branch, changed files, and pending conflicts:

<!-- transcript:status -->
```console
$ ff status --json | jq '.data | {head, changes, held, resolving}'
{
  "head": {
    "state": "branch",
    "name": "main",
    "ref": "refs/heads/main",
    "commit": "afb81aa6ea08a7e62808b608e2da7b34fd221f87"
  },
  "changes": [
    {
      "path": "main.rs",
      "from": null,
      "kind": "modified",
      "insertions": 1,
      "deletions": 1,
      "binary": false
    }
  ],
  "held": null,
  "resolving": null
}
```
<!-- /transcript -->

Other useful fields in `data`:

| Fields | Meaning |
| --- | --- |
| `root`, `worktree` | Canonical checkout root and worktree identity; `worktree.main` names the main checkout root. |
| `base`, `remote`, `upstream` | Recorded base, selected remote, and remote-tracking comparison when available. Base includes name, ref, tip, role, and commits above it; it is null on trunk, detached/unborn HEAD, and inside an editing session. |
| `changes`, `insertions`, `deletions` | Uncommitted paths and counts. Rename/copy entries carry their source in `from`; `binary` distinguishes non-line-count changes. |
| `open` | Current open state. `id` is its capture operation, `change_id` its change identity, and `pending` its internal description commit. `clean` compares its tree with the base. |
| `parent` | Commit beneath open work, with its SHA, change ID, and `segment` capture when found on this chain. |
| `futures` | Base and remote pull comparisons, each naming what was compared and the verdict. |
| `foreign`, `held`, `resolving`, `session` | Outside changes, a held replay, resolution information, or editing-session context. These require interpretation, not an automatic assumption that all need human intervention. |
| `last_op` | Last operation preceding this status read's own capture, using the op-log row shape. Foreign movement is reconciled before it is selected. |

## `ff log --json`

[`ff log`](../reference/cli/log.md) returns recorded commits plus a separate open block. This projection keeps the one-commit list whole and three open-state identifiers; `jq .` only indents the line:

<!-- transcript:log -->
```console
$ ff log -n 1 --json --fields commits,open.id,open.change_id,open.pending | jq .
{
  "ff": 1,
  "cmd": "log",
  "data": {
    "commits": [
      {
        "id": "50562eaedd69e7745ec985f1645cf96ffd8c0d37",
        "short_id": "50562eae",
        "change_id": "urxppurnzxvuwnukttzwlxtsurkywqmt",
        "subject": "parser: skeleton",
        "body": "",
        "author_name": "Ada Lovelace",
        "author_email": "ada@example.com",
        "time": 1789860930,
        "signed": false,
        "session": null
      }
    ],
    "open": {
      "id": "3f90f4078f0b1c0c3263f7f32f2d163b846f806f",
      "change_id": "wrrrsrpyymttzvqqtktqrompytsqpwmn",
      "pending": "4c83f4ed9c04ddd783c292ed8c5fbeac9a0a6d71"
    }
  }
}
```
<!-- /transcript -->

A commit's `id` is its SHA; `change_id` is the identity it retains across surviving fufu rewrites. `body` is the message after its subject, empty for a one-line message. The open block's `id` is a capture operation, not a commit SHA. Its `pending` object has not yet entered branch history. A commit's `session` identifies the tag under which it was recorded. [`ff show`](../reference/cli/show.md) also reports change identity.

Under `-p`, `--stat`, or `--name-only`, each `ff log --json` row and the open block gain `changes`, `insertions`, and `deletions`, the shape `ff diff --json` and `ff show --json` carry. `--stat` drops each file's `hunks`; `--name-only` keeps `path`, `from`, `kind`, and `binary` per file and drops the counts; `ff show --no-patch` drops the three keys. Keys are dropped, never renamed or set to null. Rows under a view carry `against`, the word `ff show --json` carries for what the row was measured against: `parent`, `auto-merge`, or `first-parent`.

## `ff evolog --json`

Bare [`ff evolog`](../reference/cli/evolog.md), or evolog on `@`, returns captures of the open change, newest first:

<!-- transcript:evolog -->
```console
$ ff evolog -n 1 --json | jq '.data'
{
  "change_id": "okxorxuovysykxnowokkplmqnotvrqnr",
  "snapshots": [
    {
      "id": "0acd0a75e7636e687942d6338c7d00447f842c75",
      "short_id": "0acd",
      "subject": "pre: ff status --json",
      "time": 1789342058,
      "base": "afb81aa6ea08a7e62808b608e2da7b34fd221f87",
      "prev": "10a5a848f7fd390790eaf6d948949007daf8d630",
      "session": null
    }
  ]
}
```
<!-- /transcript -->

On a recorded revision, evolog finds operations producing commits with its change ID across worktree chains, plus captures behind its commit. This projection shows the operation side:

<!-- transcript:closed-evolog -->
```console
$ ff evolog HEAD --json | jq '.data | {change_id, commit, operations}'
{
  "change_id": "wzyzppxmrnmosplkvqqzmrxxwnsvwnlr",
  "commit": "afb81aa6ea08a7e62808b608e2da7b34fd221f87",
  "operations": [
    {
      "id": "cc3d5843216df4d6ee90f9fa617e9198b5ba56a6",
      "short_id": "cc3d",
      "chain": "main",
      "verb": "commit",
      "summary": "commit on main: parser: skeleton",
      "time": 1789342058,
      "commit": "afb81aa6ea08a7e62808b608e2da7b34fd221f87",
      "session": null
    }
  ]
}
```
<!-- /transcript -->

Each operation's `commit` is the version it produced, which can differ from the requested SHA after a rewrite. `chain` names its worktree. `-p` adds per-snapshot file changes and counts. A commit without a change-id header has an empty operations list and matching captures from the current chain.

## `ff history --json`

[`ff history`](../reference/cli/history.md) returns undo steps, not one row per operation. This projection selects the earliest-recovery-point flag and the first two steps:

<!-- transcript:history -->
```console
$ ff history --json | jq '.data | {floor, steps: .steps[:2]}'
{
  "floor": true,
  "steps": [
    {
      "id": "386753f42f6cb296325bfa1109c3cc35e08521f7",
      "short_id": "3867",
      "landing": "now",
      "kind": "capture",
      "summary": "manual",
      "time": 1789342058,
      "branch": "main",
      "session": "flight-3",
      "collapsed": 0,
      "distance": 0
    },
    {
      "id": "0acd0a75e7636e687942d6338c7d00447f842c75",
      "short_id": "0acd",
      "landing": "undo",
      "kind": "capture",
      "summary": "pre: ff status --json",
      "time": 1789342058,
      "branch": "main",
      "session": null,
      "collapsed": 2,
      "distance": 1
    }
  ]
}
```
<!-- /transcript -->

`landing` is `now`, `undo`, or `redo`. Positive `distance` counts [`ff undo`](../reference/cli/undo.md) steps; negative distance counts [`ff redo`](../reference/cli/redo.md) steps. `collapsed` counts operations grouped into the step. `data.floor` says the history reaches the initialized-from-observed-state record. Every `id` is an operation address. [Snapshots and undo](../concepts/snapshots-and-undo.md#reading-ff-history) explains grouping and recovery limits.

## Reading the operation log from a script

Op log returns individual retained records, including captures. The two newest here belong to `flight-3`:

<!-- transcript:op-log -->
```console
$ ff op log --json | jq '.data.ops[:2]'
[
  {
    "id": "386753f42f6cb296325bfa1109c3cc35e08521f7",
    "short_id": "3867",
    "kind": "capture",
    "verb": "",
    "summary": "manual",
    "time": 1789342058,
    "branch": "main",
    "session": "flight-3",
    "undo_of": null
  },
  {
    "id": "94d5cac5577c48cc00612ddea2723de52ed9fc2e",
    "short_id": "94d5",
    "kind": "capture",
    "verb": "",
    "summary": "manual",
    "time": 1789342058,
    "branch": "main",
    "session": "flight-3",
    "undo_of": null
  }
]
```
<!-- /transcript -->

`kind` distinguishes `op`, `capture`, `foreign`, and `note`. `verb` names the command for a command operation; a capture's summary describes its provenance, often with a `pre:` prefix. `undo_of` links a recorded inverse to its target when present; ordinary undo navigates the log rather than appending such a row.

[`ff op show`](../reference/cli/op-show.md) exposes ref transitions. The operation ID below comes from the commit operation in this same fixture:

<!-- transcript:op-show -->
```console
$ ff op show cc3d5843216df4d6ee90f9fa617e9198b5ba56a6 --json | jq '.data | {id, kind, summary, tree, refs}'
{
  "id": "cc3d5843216df4d6ee90f9fa617e9198b5ba56a6",
  "kind": "op",
  "summary": "commit on main: parser: skeleton",
  "tree": "5d90422423db5ef6b431e8b9e60e0baf04b8742a",
  "refs": [
    {
      "name": "refs/heads/main",
      "old": null,
      "new": "afb81aa6ea08a7e62808b608e2da7b34fd221f87"
    }
  ]
}
```
<!-- /transcript -->

[`ff op diff`](../reference/cli/op-diff.md) compares files in operation trees, not ref transitions. [`ff op restore`](../reference/cli/op-restore.md) restores recorded local state subject to worktree guards; [`ff restore`](../reference/cli/restore.md) with `--at-op` restores selected files. [`ff op revert`](../reference/cli/op-revert.md) only reverses applicable ref transitions and preserves files, index, and HEAD selection. Use the [recovery guide](../guides/recovery.md) to choose the scope.

### Replay reports

Rewrite reports and cascade entries can carry `dropped`: commits removed by replay instead of rewritten. Each entry has the old SHA, subject, and reason. `empty` means the replayed tree matched its new parent's tree. `superseded` means the base already contained that change ID, with `by` naming the replacing base commit. Inspect these alongside moved, held, and skipped outcomes.
