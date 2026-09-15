//! Copilot CLI.
//!
//! Wired through an Agent Plugins 1.0 plugin fufu owns at
//! `~/.agents/plugins/copilot/fufu/`: a root `plugin.json` under the
//! `agent-plugins.org` schema, `com.github.copilot/hooks/hooks.json` in
//! Copilot's flat shape, and the skill under `skills/fufu/`. The
//! dedicated `copilot/` namespace keeps Copilot's manifest, hooks, and
//! marketplace apart from Codex's incompatible formats one directory up.
//! Copilot finds the plugin through `<root>/marketplace.json` beside it
//! and a registration merged into `~/.copilot/settings.json` —
//! `enabledPlugins["fufu@<market>"]` and `extraKnownMarketplaces[<market>]`
//! — and loads it live, with no trust step (verified with 1.0.83).
//!
//! The marketplace file is shared with tower, which puts its own plugin
//! under the same root. fufu merges one entry, `{"name":"fufu","source":
//! "./fufu"}`, into `plugins[]` and takes the file's `name` and `owner`
//! when another tool created it, so the selector follows the file:
//! `fufu@fufu-ff` in a marketplace fufu wrote, `fufu@tower-atc` in one
//! tower did. Uninstall drops fufu's entry and removes the file and the
//! marketplace registration only when nothing else is listed.
//!
//! Copilot's payload carries `sessionId` and `cwd` and names no event; each
//! hook entry sets `FF_HOOK_EVENT` in its environment instead, and the
//! adapter reads it into the payload's `hook_event_name` when the payload
//! has none. The pre-tool payload's tool keys are read best-effort
//! (`toolName`, `toolArgs`, `toolInput`); an unrecognized shape still
//! captures under the honest `event preToolUse` label.

use std::path::{Path, PathBuf};

use ff_core::Result;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::settings::{Event, Need};
use super::{
    AgentEvent, AgentProtocol, Change, EventKind, InstallOptions, Integration, Mechanism, Presence,
    Reply, Status, Wiring, payload, plugin, settings, skill,
};

pub struct Copilot;

const NAME: &str = "fufu";
/// The marketplace fufu writes when there is none; a file another tool
/// created keeps its own name, and the selector follows it.
const MARKET: &str = "fufu-ff";
const SCHEMA: &str = "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json";
const TAIL: &str = "trigger copilot";

/// The variable each hook entry sets, since the payload names no event.
pub const EVENT_VAR: &str = "FF_HOOK_EVENT";

/// Copilot's table. `preToolUse` is the floor and `userPromptSubmitted`
/// the turn the briefing rides; the rest widen capture.
const EVENTS: [Event; 5] = [
    ("preToolUse", None, Need::Required),
    ("userPromptSubmitted", None, Need::Required),
    ("sessionStart", None, Need::Extra),
    ("agentStop", None, Need::Extra),
    ("sessionEnd", None, Need::Extra),
];

// ---- paths -----------------------------------------------------------------

fn root() -> Result<PathBuf> {
    Ok(super::home()?
        .join(".agents")
        .join("plugins")
        .join("copilot"))
}

fn config_dir() -> Result<PathBuf> {
    Ok(super::home()?.join(".copilot"))
}

fn settings_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("settings.json"))
}

fn plugin_dir(root: &Path) -> PathBuf {
    root.join(NAME)
}

fn hooks_path(root: &Path) -> PathBuf {
    plugin_dir(root)
        .join("com.github.copilot")
        .join("hooks")
        .join("hooks.json")
}

fn skill_dir(root: &Path) -> PathBuf {
    plugin_dir(root).join("skills").join(skill::NAME)
}

fn marketplace_path(root: &Path) -> PathBuf {
    root.join("marketplace.json")
}

/// `FF_COPILOT` when set, the test seam — a path that is not a file means
/// no Copilot — else `copilot` (or its Windows spellings) on `PATH`.
fn binary() -> Option<PathBuf> {
    super::client_binary("FF_COPILOT", &["copilot", "copilot.exe", "copilot.cmd"])
}

// ---- the plugin ------------------------------------------------------------

fn bodies() -> (String, String) {
    let command = super::exe_command(TAIL);
    let mut hooks = Map::new();
    for (event, ..) in EVENTS {
        hooks.insert(
            event.into(),
            serde_json::json!([{
                "type": "command", "bash": command,
                "env": { EVENT_VAR: event }, "timeoutSec": 30
            }]),
        );
    }
    let hooks = plugin::pretty(&serde_json::json!({"version": 1, "hooks": hooks}));
    let hash: String = Sha256::digest(hooks.as_bytes())
        .iter()
        .take(4)
        .map(|b| format!("{b:02x}"))
        .collect();
    let manifest = plugin::pretty(&serde_json::json!({
        "$schema": SCHEMA, "name": NAME,
        "version": format!("{}+ff.{hash}", env!("CARGO_PKG_VERSION")),
        "description": "fufu (ff) snapshots the working copy before every tool action",
        "homepage": env!("CARGO_PKG_REPOSITORY")
    }));
    (manifest, hooks)
}

/// A command with the wrong event environment still runs, but cannot
/// deliver the event it is wired under.
fn has_event(hooks: &Value, event: &str) -> bool {
    hooks["version"] == 1
        && hooks["hooks"][event].as_array().is_some_and(|entries| {
            entries.iter().any(|entry| {
                entry["type"] == "command"
                    && entry["bash"].as_str().is_some_and(|c| c.ends_with(TAIL))
                    && entry["env"][EVENT_VAR] == event
            })
        })
}

fn read_json(path: &Path) -> Result<Value> {
    settings::load(path).map(Value::Object)
}

// ---- the marketplace -------------------------------------------------------

fn marketplace_entry() -> Value {
    serde_json::json!({ "name": NAME, "source": "./fufu" })
}

/// The marketplace as a map: the file as found, or the one fufu writes
/// when there is none.
fn load_marketplace(root: &Path) -> Result<Map<String, Value>> {
    let path = marketplace_path(root);
    let mut market = settings::load(&path)?;
    if !market.contains_key("name") {
        market.insert("name".into(), MARKET.into());
    }
    if !market.contains_key("owner") {
        market.insert("owner".into(), serde_json::json!({ "name": NAME }));
    }
    Ok(market)
}

/// The marketplace's own name — the half of `fufu@<name>` the
/// registration needs — read from the file as found.
fn market_name(root: &Path) -> String {
    settings::load(&marketplace_path(root))
        .ok()
        .and_then(|market| market.get("name")?.as_str().map(str::to_string))
        .unwrap_or_else(|| MARKET.to_string())
}

fn selector(name: &str) -> String {
    format!("{NAME}@{name}")
}

/// fufu's entry, replaced in place or appended; everything else as found.
fn merge_entry(path: &Path, market: &mut Map<String, Value>) -> Result<()> {
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

/// Whether the marketplace lists fufu's plugin the way fufu writes it.
fn has_entry(market: &Value) -> bool {
    market["plugins"].as_array().is_some_and(|plugins| {
        plugins
            .iter()
            .any(|p| p["name"] == NAME && p["source"] == "./fufu")
    })
}

// ---- the registration ------------------------------------------------------

fn registration(root: &Path) -> Value {
    serde_json::json!({"source": {"source": "directory", "path": root}})
}

/// Validate both maps before editing either. Malformed foreign settings
/// are refused on install and uninstall alike.
fn load_settings(path: &Path) -> Result<Map<String, Value>> {
    let settings = settings::load(path)?;
    for key in ["enabledPlugins", "extraKnownMarketplaces"] {
        if settings.get(key).is_some_and(|value| !value.is_object()) {
            return Err(super::malformed(
                path,
                format!("\"{key}\" is not an object"),
            ));
        }
    }
    Ok(settings)
}

fn register(settings: &mut Map<String, Value>, root: &Path, name: &str) {
    for (key, entry, value) in [
        ("enabledPlugins", selector(name), Value::Bool(true)),
        (
            "extraKnownMarketplaces",
            name.to_string(),
            registration(root),
        ),
    ] {
        settings
            .entry(key)
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .expect("validated settings map")
            .insert(entry, value);
    }
}

fn registered(settings: &Map<String, Value>, root: &Path, name: &str) -> bool {
    settings
        .get("enabledPlugins")
        .and_then(|v| v.get(selector(name)))
        == Some(&Value::Bool(true))
        && settings
            .get("extraKnownMarketplaces")
            .and_then(|v| v.get(name))
            .is_some_and(|v| v["source"] == registration(root)["source"])
}

fn mentions(settings: &Map<String, Value>, name: &str) -> bool {
    settings
        .get("enabledPlugins")
        .and_then(|v| v.get(selector(name)))
        .is_some()
        || settings
            .get("extraKnownMarketplaces")
            .and_then(|v| v.get(name))
            .is_some()
}

// ---- wiring ----------------------------------------------------------------

fn wiring(root: &Path) -> Result<Wiring> {
    let dir = plugin_dir(root);
    let settings = load_settings(&settings_path()?)?;
    let market_path = marketplace_path(root);
    let name = market_name(root);
    if !dir.exists() && !market_path.exists() && !mentions(&settings, &name) {
        return Ok(Wiring::NotWired);
    }
    let manifest = read_json(&dir.join("plugin.json"))?;
    let hooks = read_json(&hooks_path(root))?;
    let market = read_json(&market_path)?;
    let mut missing = Vec::new();
    if manifest["$schema"] != SCHEMA || manifest["name"] != NAME || !manifest["version"].is_string()
    {
        missing.push("1.0 manifest");
    }
    if !market["name"].is_string() || !has_entry(&market) {
        missing.push("marketplace entry");
    }
    if !registered(&settings, root, &name) {
        missing.push("Copilot registration");
    }
    for (event, _, need) in &EVENTS {
        if *need == Need::Required && !has_event(&hooks, event) {
            missing.push(event);
        }
    }
    Ok(if missing.is_empty() {
        Wiring::Wired {
            mechanism: Mechanism::Plugin,
            at: dir,
        }
    } else {
        Wiring::Partial {
            missing: missing.join(", "),
            at: dir,
        }
    })
}

fn stale(root: &Path) -> bool {
    read_json(&hooks_path(root)).is_ok_and(|hooks| {
        EVENTS.iter().any(|(e, ..)| has_event(&hooks, e))
            && EVENTS
                .iter()
                .any(|(e, _, need)| *need == Need::Extra && !has_event(&hooks, e))
    })
}

// ---- the integration -------------------------------------------------------

impl Integration for Copilot {
    fn slug(&self) -> &'static str {
        "copilot"
    }

    fn detect(&self) -> Presence {
        if let Ok(dir) = config_dir()
            && dir.is_dir()
        {
            return Presence::Present { evidence: dir };
        }
        binary().map_or(Presence::Absent, |evidence| Presence::Present { evidence })
    }

    fn status(&self) -> Status {
        let root = root();
        let wiring = root
            .as_ref()
            .map_err(|err| err.to_string())
            .and_then(|root| wiring(root).map_err(|err| err.to_string()))
            .unwrap_or_else(|complaint| Wiring::Unavailable { complaint });
        let skill = root
            .as_ref()
            .map_or(Wiring::NotWired, |root| skill::wiring(&skill_dir(root)));
        let stale = root.as_ref().is_ok_and(|root| stale(root));
        Status {
            slug: self.slug(),
            presence: self.detect(),
            wiring,
            note: None,
            parts: Vec::new(),
            skill: Some(skill),
            stale,
        }
    }

    fn install(&self, _opts: &InstallOptions) -> Result<Change> {
        let root = root()?;
        let dir = plugin_dir(&root);
        let path = settings_path()?;
        let before = load_settings(&path)?;
        let market_path = marketplace_path(&root);
        let market_before = settings::load(&market_path)?;
        let mut market = load_marketplace(&root)?;
        merge_entry(&market_path, &mut market)?;
        let name = market["name"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| MARKET.to_string());
        let mut settings = before.clone();
        register(&mut settings, &root, &name);
        let (manifest, hooks) = bodies();
        let mut changed = plugin::write_if_changed(&dir.join("plugin.json"), &manifest)?;
        changed |= plugin::write_if_changed(&hooks_path(&root), &hooks)?;
        let skills = skill_dir(&root);
        if !matches!(skill::wiring(&skills), Wiring::Wired { .. }) {
            skill::write(&skills)?;
            changed = true;
        }
        if market != market_before || !market_path.exists() {
            settings::write(&market_path, &market)?;
            changed = true;
        }
        if settings != before {
            settings::write(&path, &settings)?;
            changed = true;
        }
        let verified = wiring(&root)?;
        if !matches!(verified, Wiring::Wired { .. }) {
            return Err(super::failed(
                &dir,
                format!(
                    "the plugin did not verify after the write ({})",
                    verified.word()
                ),
            ));
        }
        let line = format!(
            "{} in {}; registered live as {}",
            if changed {
                "plugin and skill written"
            } else {
                "already wired"
            },
            dir.display(),
            selector(&name)
        );
        Ok(if changed {
            Change::changed(line)
        } else {
            Change::unchanged(line)
        })
    }

    /// Exactly what install wrote: the plugin directory, fufu's entry in
    /// the marketplace, and the registration — the marketplace file and
    /// its `extraKnownMarketplaces` entry only when nothing else is
    /// listed there, since another tool's plugin may share the root.
    fn uninstall(&self, _opts: &InstallOptions) -> Result<Change> {
        let root = root()?;
        let path = settings_path()?;
        let mut settings = load_settings(&path)?;
        let market_path = marketplace_path(&root);
        let name = market_name(&root);
        let mut changed = false;
        let mut market_stays = false;
        if market_path.exists() {
            let mut market = settings::load(&market_path)?;
            if let Some(plugins) = market.get_mut("plugins").and_then(Value::as_array_mut) {
                let before = plugins.len();
                plugins.retain(|p| p["name"] != NAME);
                changed |= plugins.len() != before;
                market_stays = !plugins.is_empty();
            }
            if market_stays {
                settings::write(&market_path, &market)?;
            } else {
                std::fs::remove_file(&market_path)
                    .map_err(|err| super::failed(&market_path, err))?;
                changed = true;
            }
        }
        let mut settings_changed = false;
        if let Some(map) = settings
            .get_mut("enabledPlugins")
            .and_then(Value::as_object_mut)
        {
            settings_changed |= map.remove(&selector(&name)).is_some();
        }
        if !market_stays
            && let Some(map) = settings
                .get_mut("extraKnownMarketplaces")
                .and_then(Value::as_object_mut)
        {
            settings_changed |= map.remove(&name).is_some();
        }
        if settings_changed {
            settings::write(&path, &settings)?;
            changed = true;
        }
        let dir = plugin_dir(&root);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|err| super::failed(&dir, err))?;
            changed = true;
        }
        Ok(if changed {
            Change::changed(format!(
                "removed {} and registration {}",
                dir.display(),
                selector(&name)
            ))
        } else {
            Change::unchanged("no fufu plugin installed")
        })
    }

    fn protocol(&self) -> Option<&'static dyn AgentProtocol> {
        Some(&Copilot)
    }
}

impl AgentProtocol for Copilot {
    /// The shared dialect with the event read from the environment when
    /// the payload names none, which Copilot's never does.
    fn parse(&self, stdin: &[u8], forced: Option<EventKind>) -> Result<Option<AgentEvent>> {
        let mut payload: payload::Payload = payload::parse_json(stdin)?;
        if payload.hook_event_name.is_empty()
            && let Some(name) = std::env::var_os(EVENT_VAR)
        {
            payload.hook_event_name = name.to_string_lossy().into_owned();
        }
        payload::to_event(&payload, forced)
    }

    /// Copilot reads injected context out of a JSON field, and documents
    /// no channel on a tool.
    fn reply_envelope(&self, reply: &Reply) -> Option<String> {
        if reply.kind == EventKind::BeforeTool || reply.context.is_empty() {
            return None;
        }
        Some(serde_json::json!({ "additionalContext": reply.joined() }).to_string())
    }

    fn has_skill(&self) -> bool {
        root().is_ok_and(|root| skill::installed(&skill_dir(&root)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 1.0 manifest under its schema with the hash-suffixed version;
    /// the flat hooks file with every event's command, environment, and
    /// timeout; and each reading back by its own event.
    #[test]
    fn the_bodies_are_the_manifest_and_the_flat_hooks() {
        let (manifest, hooks) = bodies();
        let manifest: Value = serde_json::from_str(&manifest).unwrap();
        assert_eq!(manifest["$schema"], SCHEMA);
        assert_eq!(manifest["name"], "fufu");
        let version = manifest["version"].as_str().unwrap();
        let (pkg, hash) = version.split_once("+ff.").unwrap();
        assert_eq!(pkg, env!("CARGO_PKG_VERSION"));
        assert_eq!(hash.len(), 8);
        let hooks: Value = serde_json::from_str(&hooks).unwrap();
        assert_eq!(hooks["version"], 1);
        assert_eq!(hooks["hooks"].as_object().unwrap().len(), EVENTS.len());
        for (event, ..) in EVENTS {
            assert!(has_event(&hooks, event), "{event}: {hooks}");
            let entry = &hooks["hooks"][event][0];
            assert!(entry["bash"].as_str().unwrap().starts_with('"'));
            assert_eq!(entry["timeoutSec"], 30);
            assert!(entry.get("hooks").is_none(), "flat, not nested: {entry}");
        }
        assert!(!has_event(&hooks, "nonsense"));
    }

    /// The marketplace fufu creates carries its own name and owner; one
    /// tower created keeps both, and fufu's entry joins its list.
    #[test]
    fn the_marketplace_entry_joins_a_foreign_file() {
        let path = Path::new("marketplace.json");
        let mut market = serde_json::json!({
            "name": "tower-atc", "owner": { "name": "tower" },
            "plugins": [{ "name": "tower", "source": "./tower" }]
        })
        .as_object()
        .unwrap()
        .clone();
        merge_entry(path, &mut market).unwrap();
        assert_eq!(market["name"], "tower-atc");
        assert_eq!(market["plugins"].as_array().unwrap().len(), 2);
        assert_eq!(market["plugins"][1]["source"], "./fufu");
        assert!(has_entry(&Value::Object(market.clone())));
        merge_entry(path, &mut market).unwrap();
        assert_eq!(market["plugins"].as_array().unwrap().len(), 2, "in place");
        assert_eq!(selector("tower-atc"), "fufu@tower-atc");

        let mut bad = serde_json::json!({ "plugins": 1 })
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(
            merge_entry(path, &mut bad).unwrap_err().id(),
            "hook/malformed"
        );
    }

    #[test]
    fn the_briefing_is_json_wrapped_and_a_tool_gets_nothing() {
        let mut reply = Reply::new(EventKind::ContextStart);
        reply.context.push("hello".into());
        let out = Copilot.reply_envelope(&reply).expect("a turn speaks");
        let value: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value["additionalContext"], "hello");
        assert_eq!(value.as_object().unwrap().len(), 1);

        let mut reply = Reply::new(EventKind::BeforeTool);
        reply.context.push("hello".into());
        assert!(Copilot.reply_envelope(&reply).is_none());
    }
}
