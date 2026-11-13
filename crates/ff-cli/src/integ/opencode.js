// Written by `ff hook opencode`. Rewritten by `ff hook -u`, removed by `ff unhook opencode`.
// The whole file is fufu's: a byte changed reads as stale, and `ff hook -u` rewrites it.

import { spawn } from "node:child_process";

const FF = __FF__;

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
    // No shell hook: 2.x's shell tool sets OPENCODE_SESSION_ID itself.
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
