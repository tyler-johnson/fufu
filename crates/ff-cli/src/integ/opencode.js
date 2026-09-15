// Written by `ff hook opencode`. Rewritten by `ff hook -u`, removed by `ff unhook opencode`.
// The whole file is fufu's: a byte changed reads as stale, and `ff hook -u` rewrites it.
const FF = __FF__;

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
