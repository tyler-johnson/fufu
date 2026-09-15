//! Codex.
//!
//! Wired through a plugin fufu owns outright, the way `claude.rs` wires
//! Claude Code: one directory at `~/.agents/plugins/fufu/`, written whole
//! and removed whole — manifest, `hooks/hooks.json` in the nested shape
//! every event in the table maps to, and the shipped skill. Codex
//! discovers it through the personal marketplace beside it,
//! `~/.agents/plugins/marketplace.json`, a file fufu does not own:
//! install merges one entry under `plugins` and leaves the rest as found.
//! A local `source.path` there resolves against the marketplace root, the
//! directory holding `.agents/` — HOME — so the entry is spelled
//! `./.agents/plugins/fufu`.
//!
//! The manifest is Codex's legacy one, `.codex-plugin/plugin.json`, on
//! purpose. Codex 0.153.4 (and main at 0.154.0) also reads the Agent
//! Plugins 1.0 manifest — a root `plugin.json` under the
//! `agent-plugins.org` schema — but loads skills and MCP servers from it
//! alone and never its hooks (`core-plugins/src/loader.rs`, the
//! `PluginManifestFormat::AgentPlugin` arm), and when both manifests are
//! present the 1.0 one wins. So a plugin that is to capture carries the
//! legacy manifest and no root `plugin.json`. The standard's directory is
//! still the home, since it is where Codex looks without being told; the
//! 1.0 manifest is a byte change once Codex runs hooks from it.
//!
//! `INSTALLED_BY_DEFAULT` on the marketplace entry drives labels alone in
//! Codex; every install goes through `codex plugin add fufu@<name>`, which
//! install spawns when `codex` is on `PATH` and otherwise names. Codex
//! copies an installed plugin into a cache keyed by version, so the
//! manifest's `version` is the crate's plus a hash of the hooks file — a
//! rewritten table or a moved binary is a new version, and the add runs
//! again. Plugin hooks are untrusted until `/hooks` reviews them, and the
//! report says so; the shipped skill is a file Codex reads, not a command
//! it runs, and needs no trust.
//!
//! The payload is field-for-field Claude Code's, down to aliasing
//! `CLAUDE_PLUGIN_ROOT`, so it shares the dialect and the runtime.
//!
//! The migration off the settings entries an older fufu wrote in
//! `~/.codex/hooks.json`, with the skill beside them in `~/.codex/skills/`
//! and the MCP block in `config.toml`, is add-then-remove, the Claude
//! adapter's rule: write the plugin, verify it reads back, then strip —
//! each best-effort, since the plugin already captures. `ff trigger codex`
//! is the command both spellings run.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use ff_core::Result;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::{
    AgentEvent, AgentProtocol, Change, EventKind, InstallOptions, Integration, Presence, Reply,
    Status, Wiring, mcp, payload, plugin, settings, skill,
};
use settings::{Event, Need};

pub struct Codex;

const NAME: &str = "fufu";

/// The plugin bakes an absolute path, so recognizing our own wiring cannot
/// be an equality test: the binary moves, and a moved binary must still
/// read as wired rather than as gone. No legacy tail: the plugin never
/// carried an older spelling — `ff trigger codex` sits in the settings
/// file the migration strips, and `OLD` knows it.
const TAIL: &str = "trigger codex";

/// Codex's table. The first two found the floor and are required:
/// `PreToolUse` is the one capture cannot miss, and `UserPromptSubmit` is
/// the turn the briefing rides. The rest widen capture rather than found
/// it, so an install predating them is stale and not partial:
/// `SessionStart` sources startup, resume, clear, and compact, so the one
/// event covers every context boundary; `Stop` makes the last edit of a
/// turn durable; `SessionEnd` is the last look.
const EVENTS: [Event; 5] = [
    ("PreToolUse", Some("Bash|apply_patch"), Need::Required),
    ("UserPromptSubmit", None, Need::Required),
    ("SessionStart", None, Need::Extra),
    ("Stop", None, Need::Extra),
    ("SessionEnd", None, Need::Extra),
];

const TRUST: &str = "Codex trusts a hook by its hash: run /hooks in Codex to review this one, \
                     or it is skipped and nothing captures";

/// The settings file an older fufu wrote, under HOME, and the command it
/// wrote there — kept so the migration can strip it. The file is
/// rewritten only when someone runs the installer again.
const OLD_HOOKS: &str = ".codex/hooks.json";
const OLD_COMMAND: &str = "ff trigger codex";
const OLD_LEGACY: [&str; 0] = [];

/// Where the older fufu wrote the skill, under HOME.
const OLD_SKILL: &str = ".codex/skills/fufu";

fn old_spec() -> Result<settings::Spec> {
    Ok(settings::Spec {
        path: super::home()?.join(OLD_HOOKS),
        shape: settings::Shape::Nested,
        events: &EVENTS,
        command: OLD_COMMAND.into(),
        legacy: &OLD_LEGACY,
        version: None,
    })
}

/// Where a fufu before v0.15 registered its MCP server: a marked block in
/// `config.toml`, the one TOML file among the clients.
fn mcp_spec() -> Result<mcp::Spec> {
    Ok(mcp::Spec {
        path: config_dir()?.join("config.toml"),
        shape: mcp::Shape::TomlBlock,
    })
}

// ---- paths -----------------------------------------------------------------

fn config_dir() -> Result<PathBuf> {
    Ok(super::home()?.join(".codex"))
}

fn plugins_root() -> Result<PathBuf> {
    Ok(super::home()?.join(".agents").join("plugins"))
}

fn plugin_dir() -> Result<PathBuf> {
    Ok(plugins_root()?.join(NAME))
}

/// The legacy manifest, the one Codex loads hooks from.
fn manifest_path() -> Result<PathBuf> {
    Ok(plugin_dir()?.join(".codex-plugin").join("plugin.json"))
}

fn hooks_path() -> Result<PathBuf> {
    Ok(plugin_dir()?.join("hooks").join("hooks.json"))
}

/// Where the plugin's skill lives: `skills/fufu/`, `$fufu` in a session.
fn skill_dir() -> Result<PathBuf> {
    Ok(plugin_dir()?.join("skills").join(skill::NAME))
}

fn marketplace_path() -> Result<PathBuf> {
    Ok(plugins_root()?.join("marketplace.json"))
}

fn skill_wiring() -> Wiring {
    match skill_dir() {
        Ok(dir) => skill::wiring(&dir),
        Err(_) => Wiring::NotWired,
    }
}

fn is_ours(command: &str) -> bool {
    command.ends_with(TAIL)
}

// ---- the plugin ------------------------------------------------------------

/// The manifest's version: the crate's, plus eight hex of the hooks file's
/// SHA-256, so a rewritten table or a moved binary is a new version to
/// Codex's cache.
fn version(hooks: &str) -> String {
    let digest = Sha256::digest(hooks.as_bytes());
    let hash8: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
    format!("{}+ff.{hash8}", env!("CARGO_PKG_VERSION"))
}

fn plugin_body() -> (String, String) {
    let hooks = plugin::hooks_body(&EVENTS, &super::exe_command(TAIL));
    let manifest = serde_json::json!({
        "name": NAME,
        "version": version(&hooks),
        "description": "fufu (ff) snapshots the working copy before every tool action",
        "homepage": env!("CARGO_PKG_REPOSITORY"),
    });
    (plugin::pretty(&manifest), hooks)
}

fn plugin_stale() -> bool {
    let Ok(path) = hooks_path() else {
        return false;
    };
    plugin::stale(&path, &EVENTS, is_ours, |_| false)
}

fn plugin_wiring() -> Wiring {
    let (Ok(path), Ok(dir)) = (hooks_path(), plugin_dir()) else {
        return Wiring::NotWired;
    };
    plugin::wiring(&path, &dir, &EVENTS, is_ours)
}

/// Writes the plugin whole: manifest, hooks, and the skill, each only when
/// its bytes differ. Answers whether any byte moved.
fn write_plugin() -> Result<bool> {
    let (manifest, hooks) = plugin_body();
    let mut changed = plugin::write_if_changed(&manifest_path()?, &manifest)?;
    changed |= plugin::write_if_changed(&hooks_path()?, &hooks)?;
    let dir = skill_dir()?;
    if !matches!(skill::wiring(&dir), Wiring::Wired { .. }) {
        skill::write(&dir)?;
        changed = true;
    }
    Ok(changed)
}

fn remove_plugin() -> Result<bool> {
    let dir = plugin_dir()?;
    if !dir.exists() {
        return Ok(false);
    }
    std::fs::remove_dir_all(&dir).map_err(|err| super::failed(&dir, err))?;
    Ok(true)
}

// ---- the marketplace -------------------------------------------------------

/// fufu's entry. A local `source.path` resolves against the marketplace's
/// root, the directory holding `.agents/` — HOME — so the path is spelled
/// from there.
fn marketplace_entry() -> Value {
    serde_json::json!({
        "name": NAME,
        "source": { "source": "local", "path": "./.agents/plugins/fufu" },
        "policy": { "installation": "INSTALLED_BY_DEFAULT", "authentication": "ON_INSTALL" },
        "category": "Developer Tools",
    })
}

/// Merge fufu's entry into `market`, the personal marketplace as a map:
/// `name` set only when the file has none, `plugins` created when absent,
/// the entry named `fufu` replaced in place or appended.
fn merge_entry(path: &Path, market: &mut Map<String, Value>) -> Result<()> {
    if !market.contains_key("name") {
        market.insert("name".into(), NAME.into());
    }
    let plugins = market
        .entry("plugins".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let plugins = plugins
        .as_array_mut()
        .ok_or_else(|| super::malformed(path, "\"plugins\" is not an array"))?;
    let entry = marketplace_entry();
    match plugins.iter_mut().find(|p| p["name"] == NAME) {
        Some(slot) => *slot = entry,
        None => plugins.push(entry),
    }
    Ok(())
}

/// Drop the entry named `fufu`; everything else stays as found. Answers
/// whether anything moved.
fn drop_entry(market: &mut Map<String, Value>) -> bool {
    let Some(plugins) = market.get_mut("plugins").and_then(Value::as_array_mut) else {
        return false;
    };
    let before = plugins.len();
    plugins.retain(|p| p["name"] != NAME);
    plugins.len() != before
}

/// The marketplace entry, written only when the bytes would change.
fn marketplace_install() -> Result<bool> {
    let path = marketplace_path()?;
    let before = settings::load(&path)?;
    let mut market = before.clone();
    merge_entry(&path, &mut market)?;
    if market == before && path.exists() {
        return Ok(false);
    }
    settings::write(&path, &market)?;
    Ok(true)
}

fn marketplace_uninstall() -> Result<bool> {
    let path = marketplace_path()?;
    if !path.exists() {
        return Ok(false);
    }
    let mut market = settings::load(&path)?;
    if !drop_entry(&mut market) {
        return Ok(false);
    }
    settings::write(&path, &market)?;
    Ok(true)
}

/// The marketplace's own name — the half of `fufu@<name>` the add needs —
/// read from the file as found.
fn marketplace_name() -> Option<String> {
    let path = marketplace_path().ok()?;
    let market = settings::load(&path).ok()?;
    market.get("name")?.as_str().map(str::to_string)
}

// ---- codex -----------------------------------------------------------------

/// The `codex` to spawn: `FF_CODEX` when set, the test seam — a path that
/// does not exist means no Codex — else `codex` (or `codex.exe`) found on
/// `PATH`.
fn codex_binary() -> Option<PathBuf> {
    super::client_binary("FF_CODEX", &["codex", "codex.exe"])
}

/// Whether `codex plugin list` shows `fufu@<name>` installed.
fn codex_has(codex: &Path, selector: &str) -> bool {
    let Ok(out) = Command::new(codex)
        .args(["plugin", "list"])
        .stdin(Stdio::null())
        .output()
    else {
        return false;
    };
    String::from_utf8_lossy(&out.stdout).lines().any(|line| {
        line.strip_prefix(selector)
            .is_some_and(|rest| rest.starts_with(char::is_whitespace))
            && line.contains("installed")
            && !line.contains("not installed")
    })
}

/// `codex plugin add fufu@<name>`, when Codex is here and the plugin moved
/// or is not yet installed. Never an error: the line says what ran, or
/// what to run by hand.
fn register_with_codex(name: &str, moved: bool) -> String {
    let selector = format!("{NAME}@{name}");
    let by_hand = format!("codex plugin add {selector}");
    let Some(codex) = codex_binary() else {
        return format!("Codex is not on PATH — run: {by_hand}");
    };
    if !moved && codex_has(&codex, &selector) {
        return format!("already registered with Codex as {selector}");
    }
    match Command::new(&codex)
        .args(["plugin", "add", &selector])
        .stdin(Stdio::null())
        .output()
    {
        Ok(out) if out.status.success() => format!("registered with Codex: {by_hand}"),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let why = stderr
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .unwrap_or("no output");
            format!("codex plugin add failed ({why}) — run it by hand: {by_hand}")
        }
        Err(err) => format!("codex plugin add failed ({err}) — run it by hand: {by_hand}"),
    }
}

// ---- the migration ---------------------------------------------------------

/// Strip what the settings-merge adapter wrote, each best-effort: a file
/// that will not parse is reported and left, since the plugin already
/// captures.
fn strip_old_adapter(change: &mut Change) {
    match old_spec().and_then(|spec| settings::uninstall(&spec)) {
        Ok(stripped) if stripped.changed => {
            change.absorb(Change::changed(format!("moved off ~/{OLD_HOOKS}")));
        }
        Ok(_) => {}
        Err(err) => change
            .lines
            .push(format!("left ~/{OLD_HOOKS} as found: {err}")),
    }
    if let Ok(dir) = super::home().map(|home| home.join(OLD_SKILL)) {
        match skill::remove(&dir) {
            Ok(true) => change.absorb(Change::changed(format!("removed ~/{OLD_SKILL}"))),
            Ok(false) => {}
            Err(err) => change
                .lines
                .push(format!("left ~/{OLD_SKILL} as found: {err}")),
        }
    }
    match mcp_spec().and_then(|spec| mcp::strip(&spec)) {
        Ok(stripped) => change.absorb(stripped),
        Err(err) => change
            .lines
            .push(format!("left ~/.codex/config.toml as found: {err}")),
    }
}

// ---- the integration -------------------------------------------------------

impl Integration for Codex {
    fn slug(&self) -> &'static str {
        "codex"
    }

    fn detect(&self) -> Presence {
        match config_dir() {
            Ok(dir) if dir.is_dir() => Presence::Present { evidence: dir },
            _ => Presence::Absent,
        }
    }

    fn status(&self) -> Status {
        let wiring = plugin_wiring();
        Status {
            slug: self.slug(),
            presence: self.detect(),
            // The trust step is news whenever the wiring is there: an
            // unreviewed hook and a missing one look identical from here.
            note: wiring.feeds_capture().then(|| TRUST.to_string()),
            wiring,
            parts: Vec::new(),
            skill: Some(skill_wiring()),
            stale: plugin_stale(),
        }
    }

    fn install(&self, _opts: &InstallOptions) -> Result<Change> {
        let written = write_plugin()?;
        // Verify before touching anything else: a plugin that did not land
        // must not take the old wiring down with it.
        let verified = plugin_wiring();
        if !matches!(verified, Wiring::Wired { .. }) {
            return Err(super::failed(
                &plugin_dir()?,
                format!(
                    "the plugin did not verify after the write ({}); the old wiring is left in place",
                    verified.word()
                ),
            ));
        }
        let dir = plugin_dir()?;
        let mut change = if written {
            Change::changed(format!("plugin written to {}", dir.display()))
        } else {
            Change::unchanged(format!("already wired in {}", dir.display()))
        };
        let market_moved = marketplace_install()?;
        if market_moved {
            change.absorb(Change::changed(format!(
                "marketplace entry written to {}",
                marketplace_path()?.display()
            )));
        }
        if written {
            change
                .lines
                .push(format!("skill written to {}", skill_dir()?.display()));
        }
        let name = marketplace_name().unwrap_or_else(|| NAME.to_string());
        change
            .lines
            .push(register_with_codex(&name, written || market_moved));
        // Now, and only now, the old wiring goes.
        strip_old_adapter(&mut change);
        change.lines.push(TRUST.into());
        Ok(change)
    }

    fn uninstall(&self, _opts: &InstallOptions) -> Result<Change> {
        let name = marketplace_name().unwrap_or_else(|| NAME.to_string());
        let mut change = if remove_plugin()? {
            Change::changed(format!("removed {}", plugin_dir()?.display()))
        } else {
            Change::unchanged("no fufu plugin installed")
        };
        if marketplace_uninstall()? {
            change.absorb(Change::changed(format!(
                "removed the fufu entry from {}",
                marketplace_path()?.display()
            )));
        }
        if change.changed {
            change.lines.push(format!(
                "Codex forgets it on the next start; codex plugin remove {NAME}@{name} clears its cache"
            ));
        }
        Ok(change)
    }

    fn protocol(&self) -> Option<&'static dyn AgentProtocol> {
        Some(&Codex)
    }
}

impl AgentProtocol for Codex {
    fn parse(&self, stdin: &[u8], forced: Option<EventKind>) -> Result<Option<AgentEvent>> {
        let payload: payload::Payload = payload::parse_json(stdin)?;
        payload::to_event(&payload, forced)
    }

    /// Plain stdout, the same as Claude Code — and nothing at all on a
    /// tool, because Codex documents no channel there. Naming one that is
    /// not there is worse than saying nothing.
    fn reply_envelope(&self, reply: &Reply) -> Option<String> {
        if reply.kind == EventKind::BeforeTool {
            return None;
        }
        (!reply.context.is_empty()).then(|| reply.joined())
    }

    fn has_skill(&self) -> bool {
        skill_dir().is_ok_and(|dir| skill::installed(&dir))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integ::Label;

    #[test]
    fn the_recorded_payload_parses_the_same_as_claudes() {
        let event = Codex
            .parse(
                br#"{"hook_event_name":"PreToolUse","session_id":"cx-1","cwd":"/repo",
                     "tool_name":"apply_patch","tool_input":{"file_path":"/repo/a.rs"}}"#,
                None,
            )
            .unwrap()
            .unwrap();
        assert_eq!(event.kind, EventKind::BeforeTool);
        assert_eq!(event.session, "cx-1");
        assert_eq!(
            event.label,
            Label::Path {
                tool: "apply_patch".into(),
                path: "/repo/a.rs".into()
            }
        );
    }

    #[test]
    fn the_briefing_goes_out_as_plain_text_and_a_tool_gets_nothing() {
        let mut reply = Reply::new(EventKind::ContextStart);
        reply.context.push("hello".into());
        assert_eq!(Codex.reply_envelope(&reply).as_deref(), Some("hello"));

        let mut reply = Reply::new(EventKind::BeforeTool);
        reply.context.push("hello".into());
        assert!(Codex.reply_envelope(&reply).is_none());
    }

    /// The manifest is the legacy one — no `$schema`, since the 1.0
    /// manifest would win and drop the hooks — with the name and a version
    /// carrying the hash suffix; the hooks file carries the five events,
    /// the tool matcher on `PreToolUse` alone.
    #[test]
    fn the_manifest_is_legacy_and_the_hooks_are_the_five() {
        let (manifest, hooks) = plugin_body();
        let manifest: Value = serde_json::from_str(&manifest).unwrap();
        assert!(
            manifest.get("$schema").is_none(),
            "a 1.0 manifest loads no hooks in Codex: {manifest}"
        );
        assert_eq!(manifest["name"], "fufu");
        let version = manifest["version"].as_str().unwrap();
        let (pkg, suffix) = version.split_once("+ff.").unwrap();
        assert_eq!(pkg, env!("CARGO_PKG_VERSION"));
        assert_eq!(suffix.len(), 8, "{version}");
        assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()), "{version}");
        assert_eq!(manifest.as_object().unwrap().len(), 4, "{manifest}");

        let value: Value = serde_json::from_str(&hooks).unwrap();
        assert_eq!(value["hooks"].as_object().unwrap().len(), EVENTS.len());
        for (event, matcher, _) in EVENTS {
            let entry = &value["hooks"][event][0];
            assert_eq!(
                entry.get("matcher").and_then(Value::as_str),
                matcher,
                "{event}: {entry}"
            );
            assert!(
                entry["hooks"][0]["command"].as_str().is_some_and(is_ours),
                "{event} runs fufu: {value}"
            );
        }
        assert!(plugin::missing(&value, &EVENTS, is_ours).is_empty());
    }

    /// The suffix is a function of the hooks file alone: the same bytes
    /// hash the same, and a changed table is a new version.
    #[test]
    fn the_version_suffix_follows_the_hooks_file() {
        assert_eq!(version("a"), version("a"));
        assert_ne!(version("a"), version("b"));
        assert!(version("a").starts_with(env!("CARGO_PKG_VERSION")));
    }

    /// The merge on a map: a file fufu creates gets its name; a foreign
    /// file keeps its name, its entries, and its key order, with fufu's
    /// entry appended once and replaced in place after.
    #[test]
    fn the_marketplace_entry_merges_and_drops_around_foreign_content() {
        let path = Path::new("marketplace.json");
        let mut fresh = Map::new();
        merge_entry(path, &mut fresh).unwrap();
        assert_eq!(fresh["name"], "fufu");
        assert_eq!(
            fresh["plugins"][0]["source"]["path"],
            "./.agents/plugins/fufu"
        );
        assert_eq!(
            fresh["plugins"][0]["policy"]["installation"],
            "INSTALLED_BY_DEFAULT"
        );

        let seed = serde_json::json!({
            "plugins": [{ "name": "theirs", "source": { "source": "local", "path": "./x" } }],
            "name": "mine",
            "extra": true
        });
        let mut market = seed.as_object().unwrap().clone();
        merge_entry(path, &mut market).unwrap();
        assert_eq!(market["name"], "mine", "a name that exists is left");
        let keys: Vec<&String> = market.keys().collect();
        assert_eq!(keys, vec!["plugins", "name", "extra"], "key order kept");
        let plugins = market["plugins"].as_array().unwrap();
        assert_eq!(plugins.len(), 2);
        assert_eq!(plugins[0]["name"], "theirs");
        assert_eq!(plugins[1]["name"], "fufu");

        // Replaced in place, never doubled.
        market["plugins"][1]["source"]["path"] = "./stale".into();
        merge_entry(path, &mut market).unwrap();
        let plugins = market["plugins"].as_array().unwrap();
        assert_eq!(plugins.len(), 2);
        assert_eq!(plugins[1]["source"]["path"], "./.agents/plugins/fufu");

        assert!(drop_entry(&mut market));
        assert!(!drop_entry(&mut market), "nothing left to drop");
        assert_eq!(market["plugins"].as_array().unwrap().len(), 1);
        assert_eq!(market["plugins"][0]["name"], "theirs");
        assert_eq!(market["name"], "mine");

        let mut bad = serde_json::json!({ "plugins": "nope" })
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(
            merge_entry(path, &mut bad).unwrap_err().id(),
            "hook/malformed"
        );
    }
}
