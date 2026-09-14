# ff trigger

Take a snapshot of eligible working-copy content now. With no source, or with `manual`, this is a manual capture. `-m` labels its purpose. Unchanged content creates no new capture.

## Usage

```
Usage: ff trigger [OPTIONS] [source]
```

## Examples

```sh
ff trigger                      # Snapshot now and report the result
ff trigger -m "before experiment"
ff trigger --json               # Machine-readable manual result
ff op log                       # Inspect retained snapshots
```

## Options

```
Arguments:
  [source]
          Trigger source; omitted or `manual` requests a manual snapshot

Options:
  -m <msg>
          Say what this snapshot is for

      --json
          Emit machine-readable JSON

      --fetch
          Fetch now on commands that support fetching, regardless of cadence

      --no-fetch
          Skip the fetch: read the tracking refs as they stand

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Manual results and limits

A new manual capture prints its hexadecimal operation ID and changed-file count. An unchanged tree reports that it is already snapshotted; contention reports another capture in progress. Those outcomes succeed. Other capture errors are reported normally with a nonzero exit. Warnings go to stderr.

Manual JSON includes `source`, `captured`, `op`, and `files`. `captured` is true only for a new capture. Labels are trimmed and limited to 64 characters. Automatic trimming and update-check maintenance can also run.

Snapshots exclude ignored untracked files, unsaved buffers, and content above `fufu.maxFileSize` (50 MiB by default). Older index or base content can remain for oversized tracked files. [`ff restore`](restore.md) can recover only successfully captured, retained content.

## Client and shell triggers

Other sources are integration entry points, usually invoked by installed hooks: `claude`, `codex`, `cursor`, `gemini`, and `shell`. Agent sources read client payloads from stdin; the shell source handles prompt events. Use [`ff hook`](hook.md) to install the appropriate invocation.

Client triggers exit 0 even on pipeline errors; failures are silent unless `FF_DEBUG` is set. Successful agent triggers can emit client-protocol replies containing a briefing, advice, or a strict Git-policy denial request. They are not always silent, and `--json` does not replace that protocol with the manual envelope. Unknown sources exit 0 silently.

Capture precedes agent Git-policy evaluation. Only Claude Code emits pre-tool coaching or strict-policy denial replies; enforcement depends on the client. Codex, Cursor, and Gemini capture and tally recognized writes but emit no policy reply, including under strict.
