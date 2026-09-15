# Cursor CLI verification

Verified (by tower, whose adapter this is a port of) on Linux arm64 on September 13, 2026 with Cursor CLI `2026.09.10-fd3934a`, installed through `curl https://cursor.com/install -fsS | bash`. The installer provides `agent` and `cursor-agent` symlinks to the same versioned installation. The CLI was authenticated for the session probes.

## Discovery and format

The CLI discovers `~/.cursor/plugins/local/<name>/` automatically on a new session, without `--plugin-dir`, a marketplace file, an enablement setting, or a plugin installation command. Its marketplace commands manage account-backed Git repositories. fufu uses the user-local discovery path and writes no unused marketplace entry. The native manifest is `.cursor-plugin/plugin.json`; `hooks/hooks.json` is discovered by default and uses `{"version":1,"hooks":{"sessionStart":[{"command":"..."}]}}`. Skills load from `skills/<name>/SKILL.md`.

The installed `../cursor-plugins/dist/index.js` loader also accepts a root `plugin.json` with `$schema: https://agent-plugins.org/schemas/1.0.0/plugin.schema.json` and discovers its hooks. This was checked both with and without an explicit `hooks` field. An authenticated session with native and standard probe plugins returned the injected sentence twice. This differs from the current public docs, which describe standard plugins as skills/MCP only. fufu uses the documented native manifest.

`loader.cjs` exposes the installed webpack loader without starting the authenticated CLI entry point. `tests/live_cursor.rs` uses it to verify user-local discovery, parsed hooks, the shipped skill, and removal in a scratch HOME. No model or credentials are used in CI. The loader export and bundle entry are pinned-client details; a changed bundle fails the test and must be reviewed.

## Hooks and identity

`capture.jsonl` is the hook stdin and selected environment from session `64c76817-e1c3-4d91-baa0-a420ccc49c2e`. Only the email is redacted; the probe records an explicit environment allowlist and never records credentials. `probe.py` is the recording script with its output path made configurable for reuse.

The candidate table contained sessionStart, beforeSubmitPrompt, preToolUse, stop, and sessionEnd. The plugin-only session fired sessionStart, preToolUse, and sessionEnd. The installed `1931.index.js` checks user/project hooks before invoking beforeSubmitPrompt and stop, while its start/end paths also call `hasHooksForStep`; fufu therefore wires the three verified events. A resumed chat skips sessionStart and retains its saved context.

All captured hook payloads include `hook_event_name`, `session_id`, `conversation_id`, and `workspace_roots`. sessionStart and sessionEnd omit cwd; a shell preToolUse has an empty cwd. The hook process runs in the plugin directory, so fufu uses the first nonempty workspace root when cwd is empty. The hook environment has CURSOR_PLUGIN_ROOT and CLAUDE_PLUGIN_ROOT, but no CURSOR_AGENT or CURSOR_CONVERSATION_ID. The shell has CURSOR_AGENT=1 and CURSOR_CONVERSATION_ID equal to the hook's session_id, plus a per-request CURSOR_REQUEST_ID. No client pid variable was observed.

## Trust and delivery

Running the print session before trusting the workspace exits with "Workspace Trust Required" and names `--trust`, `--yolo`, or `-f`. After `--trust`, the local plugin hook fires without a separate hook-hash review. Shell execution in print mode additionally needed `--force`. Team policy can disable local plugin imports; the adapter reports that condition alongside workspace trust. User-local hooks are not distributed to cloud agents.

The authenticated probe command was `cursor-agent --trust --force --print --output-format json 'Quote the probe notice injected at session start verbatim, then run exactly python3 probe.py --shell once. Do not edit files or use other tools.'`. The hook replied `{"additional_context":"fufu probe notice: the violet kestrel landed on runway 158."}`. The successful client result quoted that sentence verbatim and included the shell capture reproduced in `capture.jsonl`.

## Installed adapter round trip

`round-trip.json` records what the live suite expects of a fresh authenticated session in an empty repository after `ff hook cursor`: the quoted briefing, one `ff op log --json -n 1` run by the Shell tool whose output carries the capture the plugin's `preToolUse` hook took before it, and the loader's empty answer after `ff unhook cursor`. The captured summary names the source and the first eight characters of the native conversation id.
