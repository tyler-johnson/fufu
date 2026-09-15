<a id="ff-hook-opencode"></a>
# OpenCode

[`ff hook opencode`](../cli/hook.md) installs a snapshot plugin and the shipped skill for OpenCode.

## Install

```sh
ff hook opencode
```

## Activate

Restart OpenCode to load the plugin. The plugin bakes in the path of the `ff` that installed it, so OpenCode's PATH does not matter.

## Verify

```sh
ff hook -l
ff doctor --no-fetch
```

These inspect installed files only. Confirm real capture with the [capture-and-recovery check](../../agents/setup.md#verify).

<a id="what-it-writes"></a>
## Files changed

OpenCode extends through JavaScript plugin modules under its config directory rather than a hooks file. The installer writes one file fufu owns whole, `plugins/fufu.js`, and the skill under `skills/fufu/`, both under `$XDG_CONFIG_HOME/opencode` when that variable is set and `~/.config/opencode` otherwise. `opencode.json` is not touched. Other files in either directory are left alone.

```console
$ ff hook opencode
opencode plugin written to ~/.config/opencode/plugins/fufu.js
  skill written to ~/.config/opencode/skills/fufu
  the briefing is standing: in the system prompt on every model call
  restart OpenCode to load it

$ find ~/.config/opencode -type f | sort
~/.config/opencode/plugins/fufu.js
~/.config/opencode/skills/fufu/SKILL.md

$ cat ~/.config/opencode/plugins/fufu.js
// Written by `ff hook opencode`. Rewritten by `ff hook -u`, removed by `ff unhook opencode`.
// The whole file is fufu's: a byte changed reads as stale, and `ff hook -u` rewrites it.
const FF = "/usr/local/bin/ff";

export const FufuPlugin = async ({ $, directory }) => ({
  // The one variable is both the session tag and the client marker.
  "shell.env": async (input, output) => {
    if (input.sessionID) output.env.OPENCODE_SESSION_ID = input.sessionID;
  },
  // The briefing, standing: in the system prompt of every model call, so it
  // survives compaction. The trigger captures too, and an unchanged tree
  // captures nothing. A trigger that fails or says nothing puts nothing in
  // the prompt.
  "experimental.chat.system.transform": async (input, output) => {
    const payload = JSON.stringify({ hook_event_name: "SessionStart", session_id: input.sessionID ?? "", cwd: directory });
    const text = await $`echo ${payload} | ${FF} trigger opencode`.cwd(directory).nothrow().text();
    if (text.trim()) output.system.push(text.trim());
  },
  // The floor: a snapshot before every tool call, nothing printed. The
  // bash tool's args carry `command`, which is what the label and the
  // gitPolicy tally read.
  "tool.execute.before": async (input, output) => {
    const payload = JSON.stringify({ hook_event_name: "PreToolUse", session_id: input.sessionID, cwd: directory, tool_name: input.tool, tool_input: output.args ?? {} });
    await $`echo ${payload} | ${FF} trigger opencode`.cwd(directory).nothrow().quiet();
  },
});
```

The plugin registers three hooks. `tool.execute.before` attempts capture before every tool call with the tool's name and arguments; OpenCode's `bash` tool carries `command`, which is what the snapshot's label and the Git-policy tally read. OpenCode discards that hook's output, so no pre-tool coaching or denial reaches the model, including under strict policy. `experimental.chat.system.transform` runs on every model call and pushes the trigger's stdout onto the system prompt: the payload spells `SessionStart`, so the briefing is standing — present on every call and surviving compaction — and the capture it attempts adds nothing on an unchanged tree. `shell.env` sets `OPENCODE_SESSION_ID` on every shell command, so an `ff` run from the agent's shell carries the same session its hook captures do.

Wiring is a byte comparison: the file equal to what this `ff` writes is wired; fufu's header with other bytes — a moved binary, an older fufu — is wired and stale, which `ff hook -u` rewrites; a `fufu.js` without fufu's header is someone else's, reported as hand-written and never rewritten or removed.

<a id="what-ff-unhook-opencode-removes"></a>
## Remove

[`ff unhook opencode`](../cli/unhook.md) removes the plugin file when it is fufu's and the skill directory. Restart OpenCode afterward.

```console
$ ff unhook opencode
opencode removed ~/.config/opencode/plugins/fufu.js
  removed ~/.config/opencode/skills/fufu

$ find ~/.config/opencode -type f | sort
```

<a id="notes"></a>
## Troubleshooting and migration

If the listing says hand-written, a `plugins/fufu.js` that fufu did not write is in the way: move it aside and run `ff hook opencode` again. `ff doctor --fix` and `ff hook -u` rewrite a stale plugin; neither touches a hand-written one.
