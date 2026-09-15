<a id="ff-hook-codex"></a>
# Codex

[`ff hook codex`](../cli/hook.md) installs a snapshot plugin and the shipped skill for Codex. **Run `/hooks` in Codex and approve the hook by hash; an unreviewed hook is skipped.** Repeat the review after hook changes.

## Install

```sh
ff hook codex
```

## Activate

When `codex` is on `PATH`, the installer runs `codex plugin add fufu@fufu` itself and says so; otherwise it prints the command to run by hand. Then start or restart Codex, run `/hooks`, and review and approve fufu's hook. The skill requires no command approval.

## Verify

```sh
ff hook -l
ff doctor --no-fetch
```

[`ff doctor`](../cli/doctor.md) cannot read Codex's trust state, so it always retains the review reminder. An installed-file `ok` does not prove approval or capture. Follow the [capture-and-recovery check](../../agents/setup.md#verify) in the running client.

<a id="what-it-writes"></a>
## Files changed

The installer writes the owned plugin directory `~/.agents/plugins/fufu/` — the legacy `.codex-plugin/plugin.json` manifest, `hooks/hooks.json`, and the skill under `skills/fufu/` — and merges one entry named `fufu` into the personal marketplace `~/.agents/plugins/marketplace.json`. The marketplace is not fufu's file: an existing name, other entries, and key order survive under the [shared ownership rules](index.md#files-changed), and a file that is not valid JSON is refused untouched. Personal files should stay outside the plugin directory.

```console
$ ff hook codex
codex plugin written to ~/.agents/plugins/fufu
  marketplace entry written to ~/.agents/plugins/marketplace.json
  skill written to ~/.agents/plugins/fufu/skills/fufu
  Codex is not on PATH — run: codex plugin add fufu@fufu
  Codex trusts a hook by its hash: run /hooks in Codex to review this one, or it is skipped and nothing captures

$ find ~/.agents -type f | sort
~/.agents/plugins/fufu/.codex-plugin/plugin.json
~/.agents/plugins/fufu/hooks/hooks.json
~/.agents/plugins/fufu/skills/fufu/SKILL.md
~/.agents/plugins/marketplace.json

$ cat ~/.agents/plugins/fufu/hooks/hooks.json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash|apply_patch",
        "hooks": [
          {
            "type": "command",
            "command": "\"/usr/local/bin/ff\" trigger codex"
          }
        ]
      }
    ],
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "\"/usr/local/bin/ff\" trigger codex"
          }
        ]
      }
    ],
    "SessionStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "\"/usr/local/bin/ff\" trigger codex"
          }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "\"/usr/local/bin/ff\" trigger codex"
          }
        ]
      }
    ],
    "SessionEnd": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "\"/usr/local/bin/ff\" trigger codex"
          }
        ]
      }
    ]
  }
}

$ cat ~/.agents/plugins/marketplace.json
{
  "name": "fufu",
  "plugins": [
    {
      "name": "fufu",
      "source": {
        "source": "local",
        "path": "./.agents/plugins/fufu"
      },
      "policy": {
        "installation": "INSTALLED_BY_DEFAULT",
        "authentication": "ON_INSTALL"
      },
      "category": "Developer Tools"
    }
  ]
}
```

The command is the absolute path of the binary that ran `ff hook`, shown here as `/usr/local/bin/ff`, plus `trigger codex`; the path is always double-quoted. The manifest's version is the fufu version plus `+ff.` and eight hex digits of the hooks file's SHA-256, so a rewritten table or a moved binary is a new plugin version to Codex's cache and `codex plugin add` runs again. The manifest is the legacy one on purpose: Codex 0.153 and 0.154 load a plugin's hooks from `.codex-plugin/plugin.json` alone, and a root Agent Plugins 1.0 `plugin.json` beside it would win and drop them.

`PreToolUse` attempts capture before Bash or apply_patch calls. `UserPromptSubmit` captures and can deliver the briefing. `SessionStart` rebriefs after a startup, resume, clear, or compaction; `Stop` captures the final edit of a turn; `SessionEnd` captures once more at the end. Codex captures and tallies recognized Git writes but returns no pre-tool coaching or denial reply, including under strict policy.

<a id="what-ff-unhook-codex-removes"></a>
## Remove

[`ff unhook codex`](../cli/unhook.md) removes the plugin directory and fufu's marketplace entry. Codex forgets the plugin on its next start; `codex plugin remove fufu@fufu` clears its cache.

```console
$ ff unhook codex
codex removed ~/.agents/plugins/fufu
  removed the fufu entry from ~/.agents/plugins/marketplace.json
  Codex forgets it on the next start; codex plugin remove fufu@fufu clears its cache

$ find ~/.agents -type f | sort
~/.agents/plugins/marketplace.json

$ cat ~/.agents/plugins/marketplace.json
{
  "name": "fufu",
  "plugins": []
}
```

An entry beside fufu's in the marketplace keeps its place, and the file keeps its name.

<a id="notes"></a>
## Troubleshooting and migration

If configuration is installed but no capture appears, check that `codex plugin list` shows `fufu@fufu` installed and that `/hooks` has approved it. `ff hook -u` repairs a plugin missing an event fufu has since added; `ff doctor --fix` repairs partial or stale managed configuration and skills; neither can approve a hook.

Before this plugin, fufu merged hook entries into `~/.codex/hooks.json` and wrote the skill to `~/.codex/skills/fufu/`. `ff hook codex` writes the plugin, verifies it reads back, and then strips those: its own entries from `~/.codex/hooks.json` (foreign entries stay), the old skill directory (other skills under `~/.codex/skills/` stay), and the marked `[mcp_servers.fufu]` block a fufu before v0.15 wrote into `~/.codex/config.toml` (an unmarked table is left alone). Each strip is best-effort and reported; a file that will not parse is named and left. The old entries no longer read as wired in `ff hook -l`, since the plugin is the mechanism now.

If the marketplace file already carries a name — `tower`, say — the plugin registers under it, and the installer prints `codex plugin add fufu@<name>` accordingly.
