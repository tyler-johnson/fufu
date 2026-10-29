<a id="ff-hook-opencode"></a>
# OpenCode

[`ff hook opencode`](../cli/hook.md) installs a snapshot plugin and the shipped skill for OpenCode.

## Install

```sh
ff hook opencode
```

## Activate

The plugin loads on OpenCode 1.18.29 and newer and on 2.x. Restart OpenCode to load it. The plugin bakes in the path of the `ff` that installed it, so OpenCode's PATH does not matter.

## Verify

```sh
ff hook -l
ff doctor --no-fetch
```

These inspect installed files only. Confirm real capture with the [capture-and-recovery check](../../agents/setup.md#verify).

<a id="what-it-writes"></a>
## Files changed

The installer writes `plugins/fufu.js` and `skills/fufu/` under `$XDG_CONFIG_HOME/opencode`, or `~/.config/opencode` when that variable is unset. The plugin file and skill directory are managed by fufu. `opencode.json` and unrelated files are left alone.

```console
$ ff hook opencode
opencode plugin written to ~/.config/opencode/plugins/fufu.js
  skill written to ~/.config/opencode/skills/fufu
  the briefing is included in the system prompt on every model call
  restart OpenCode to load it

$ find ~/.config/opencode -type f | sort
~/.config/opencode/plugins/fufu.js
~/.config/opencode/skills/fufu/SKILL.md

$ cat ~/.config/opencode/plugins/fufu.js
// Written by `ff hook opencode`. Rewritten by `ff hook -u`, removed by `ff unhook opencode`.
// The whole file is fufu's: a byte changed reads as stale, and `ff hook -u` rewrites it.

import { spawn } from "node:child_process";

const FF = "/usr/local/bin/ff";

// Runs `ff trigger opencode` with the payload on stdin and resolves to its
// stdout. No shell, so a Windows path needs no quoting. It never rejects: a
// spawn error, a nonzero exit, or a closed pipe all resolve to "".
const trigger = (payload, cwd) =>
  new Promise((resolve) => {
    let out = "";
    let child;
    try {
      child = spawn(FF, ["trigger", "opencode"], { cwd, windowsHide: true, stdio: ["pipe", "pipe", "ignore"] });
    } catch {
      return resolve("");
    }
    child.on("error", () => resolve(""));
    child.stdin.on("error", () => {});
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk) => (out += chunk));
    child.on("close", (code) => resolve(code === 0 ? out : ""));
    child.stdin.end(JSON.stringify(payload));
  });

// One default export, and no other: a named export breaks OpenCode 1.x's
// loader. 1.x (1.18.29 and newer) calls `server`; 2.x calls `setup`.
export default {
  id: "fufu",

  async server({ directory }) {
    return {
      // The one variable is both the session tag and the client marker.
      "shell.env": async (input, output) => {
        if (input.sessionID) output.env.OPENCODE_SESSION_ID = input.sessionID;
      },
      // The briefing, standing: in the system prompt of every model call, so it
      // survives compaction. The trigger captures too, and an unchanged tree
      // captures nothing. A trigger that fails or says nothing puts nothing in
      // the prompt.
      "experimental.chat.system.transform": async (input, output) => {
        const text = await trigger({ hook_event_name: "SessionStart", session_id: input.sessionID ?? "", cwd: directory }, directory);
        if (text.trim()) output.system.push(text.trim());
      },
      // The floor: a snapshot before every tool call, nothing printed. The
      // bash tool's args carry `command`, which is what the label and the
      // gitPolicy tally read.
      "tool.execute.before": async (input, output) => {
        await trigger({ hook_event_name: "PreToolUse", session_id: input.sessionID, cwd: directory, tool_name: input.tool, tool_input: output.args ?? {} }, directory);
      },
    };
  },

  async setup(ctx) {
    const cwd = ctx.location.directory;
    // No shell hook: 2.x's `create.before` carries no session id, so
    // OPENCODE_SESSION_ID is not set under 2.x.
    const registrations = [
      // The standing briefing, as above: `context` runs before every model request.
      await ctx.session.hook("context", async (event) => {
        const text = await trigger({ hook_event_name: "SessionStart", session_id: event.sessionID ?? "", cwd }, cwd);
        if (text.trim()) event.system.push({ type: "text", text: text.trim() });
      }),
      // The floor, as above.
      await ctx.tool.hook("execute.before", async (event) => {
        await trigger({ hook_event_name: "PreToolUse", session_id: event.sessionID, cwd, tool_name: event.tool, tool_input: event.input ?? {} }, cwd);
      }),
    ];
    return async () => {
      await Promise.all(registrations.map((registration) => registration.dispose()));
    };
  },
};
```

The plugin is one file with two halves, so it loads on both OpenCode lines. OpenCode 1.x calls the `server` half and 2.x calls the `setup` half:

- `tool.execute.before` (2.x: `execute.before`) attempts capture before every tool call. The tool's name and arguments supply snapshot labels and the Git-policy tally. OpenCode discards the hook's output, so no coaching or denial reaches the model, including under strict policy.
- `experimental.chat.system.transform` (2.x: `context`) includes the briefing in the system prompt on every model call, including after compaction. It also attempts capture; an unchanged tree adds no snapshot.
- `shell.env` supplies `OPENCODE_SESSION_ID` to shell commands so their fufu operations use the same session tag as hook captures. OpenCode 2.x has no equivalent, so shell commands under 2.x carry no `OPENCODE_SESSION_ID` and their fufu operations are not tagged with the session.

`ff hook -l` compares the plugin with the version this binary would write. A file with fufu's ownership header but different content is reported as stale; `ff hook -u` replaces it. A file without that header is reported as hand-written and is never rewritten or removed.

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
