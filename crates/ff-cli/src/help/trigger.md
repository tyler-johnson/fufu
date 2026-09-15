Take a snapshot of eligible working-copy content now. With no source, or with `manual`, this is a manual capture. `-m` labels its purpose. Unchanged content creates no new capture.

## Examples

```sh
ff trigger                      # Snapshot now and report the result
ff trigger -m "before experiment"
ff trigger --json               # Machine-readable manual result
ff op log                       # Inspect retained snapshots
```

### Options

### Manual results and limits

A new manual capture prints its hexadecimal operation ID and changed-file count. An unchanged tree reports that it is already snapshotted; contention reports another capture in progress. Those outcomes succeed. Other capture errors are reported normally with a nonzero exit. Warnings go to stderr.

Manual JSON includes `source`, `captured`, `op`, and `files`. `captured` is true only for a new capture. Labels are trimmed and limited to 64 characters. Automatic trimming and update-check maintenance can also run.

Snapshots exclude ignored untracked files, unsaved buffers, and content above `fufu.maxFileSize` (50 MiB by default). Older index or base content can remain for oversized tracked files. `ff restore` can recover only successfully captured, retained content.

### Client and shell triggers

Other sources are integration entry points, usually invoked by installed hooks: `claude`, `codex`, `qwen`, `opencode`, `cursor`, and `shell`. Agent sources read client payloads from stdin; the shell source handles prompt events. Use `ff hook` to install the appropriate invocation. `gemini` is a retired source: it still captures and briefs for entries an earlier fufu wrote, but `ff hook` no longer installs it.

Client triggers exit 0 even on pipeline errors; failures are silent unless `FF_DEBUG` is set. Successful agent triggers can emit client-protocol replies containing a briefing, advice, or a strict Git-policy denial request. They are not always silent, and `--json` does not replace that protocol with the manual envelope. Unknown sources exit 0 silently.

Capture precedes agent Git-policy evaluation. Only Claude Code emits pre-tool coaching or strict-policy denial replies; enforcement depends on the client. Codex, Qwen Code, OpenCode, and Cursor capture and tally recognized writes but emit no policy reply, including under strict; OpenCode discards its pre-tool hook's output, so nothing could reach the model there.
