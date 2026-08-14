//! `ff config` — read and write fufu settings stored as plain git config
//! under `fufu.*`.

use ff_core::gix::config::Source;
use ff_core::gix::config::source::Kind;
use ff_core::{Error, Result};

pub(crate) enum SettingKind {
    Size,
    Duration,
    Command,
    Cadence,
    Bool,
}

pub(crate) struct Setting {
    pub(crate) name: &'static str,
    pub(crate) key: &'static str,
    pub(crate) def: &'static str,
    pub(crate) kind: SettingKind,
    pub(crate) desc: &'static [&'static str],
}

pub(crate) fn registry() -> &'static [Setting] {
    &[
        Setting {
            name: "maxFileSize",
            key: "fufu.maxFileSize",
            def: "52428800",
            kind: SettingKind::Size,
            desc: &[
                "Largest new file a snapshot will include, in bytes (52428800 = 50 MiB).",
                "Suffixes work: 100M, 1G. Bigger files are skipped and the snapshot",
                "message lists them.",
            ],
        },
        Setting {
            name: "keep",
            key: "fufu.keep",
            def: "90d",
            kind: SettingKind::Duration,
            desc: &[
                "How long snapshots live: ff trim drops older ones, and the op journal",
                "rides the same cutoff. Compact durations (30d, 36h, 2w, 45s); a bare",
                "number means days.",
            ],
        },
        Setting {
            name: "autoTrim",
            key: "fufu.autoTrim",
            def: "1d",
            kind: SettingKind::Cadence,
            desc: &[
                "How often retention enforces itself: a trim rides an ff command at",
                "most this often, per repo. false leaves trimming entirely to",
                "`ff trim`; durations work too (12h, 2w), floored at one minute.",
            ],
        },
        Setting {
            name: "pager",
            key: "fufu.pager",
            def: "less",
            kind: SettingKind::Command,
            desc: &[
                "Pager for ff log and ff evolog on a TTY. When set it overrides FF_PAGER",
                "and PAGER; whitespace-split, no shell quoting; cat means no pager.",
            ],
        },
        Setting {
            name: "updateCheck",
            key: "fufu.updateCheck",
            def: "1d",
            kind: SettingKind::Cadence,
            desc: &[
                "How often ff looks for a new release in the background. false turns",
                "the whole machinery off (checks, notices, auto-install); true means",
                "daily; durations work too (12h, 7d, 2w), floored at one minute.",
            ],
        },
        Setting {
            name: "autoUpdate",
            key: "fufu.autoUpdate",
            def: "true",
            kind: SettingKind::Bool,
            desc: &[
                "Install new releases silently in the background. false prints a",
                "one-line notice instead; updateCheck false disables both.",
            ],
        },
    ]
}

fn lookup_key(input: &str) -> &'static Setting {
    let stripped = if input.len() >= 5 && input[..5].eq_ignore_ascii_case("fufu.") {
        &input[5..]
    } else {
        input
    };
    registry()
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(stripped))
        .unwrap_or_else(|| {
            eprintln!("ff: unknown setting \"{input}\" — `ff config` lists them all");
            std::process::exit(2);
        })
}

fn scope_label_for(
    file: &ff_core::gix::config::File,
    setting: &Setting,
) -> (Option<String>, &'static str) {
    let order = [
        (Kind::Override, "env"),
        (Kind::Repository, "local"),
        (Kind::Global, "global"),
        (Kind::System, "system"),
        (Kind::GitInstallation, "system"),
    ];
    for (k, label) in order {
        let found = file.string_filter(
            setting.key,
            &mut |md: &ff_core::gix::config::file::Metadata| md.source.kind() == k,
        );
        if let Some(v) = found {
            return (Some(v.to_string()), label);
        }
    }
    (None, "")
}

fn scope_label_excluding(
    file: &ff_core::gix::config::File,
    setting: &Setting,
    exclude: Kind,
) -> (Option<String>, &'static str) {
    let order = [
        (Kind::Override, "env"),
        (Kind::Repository, "local"),
        (Kind::Global, "global"),
        (Kind::System, "system"),
        (Kind::GitInstallation, "system"),
    ];
    for (k, label) in order {
        if k == exclude {
            continue;
        }
        let found = file.string_filter(
            setting.key,
            &mut |md: &ff_core::gix::config::file::Metadata| md.source.kind() == k,
        );
        if let Some(v) = found {
            return (Some(v.to_string()), label);
        }
    }
    (None, "")
}

fn global_config_path() -> Option<std::path::PathBuf> {
    let mut env = |n: &str| std::env::var_os(n);
    let user = Source::User.storage_location(&mut env);
    let xdg = Source::Git.storage_location(&mut env);

    if let Some(p) = &user
        && p.exists()
    {
        return Some(p.to_path_buf());
    }
    if let Some(p) = &xdg
        && p.exists()
    {
        return Some(p.to_path_buf());
    }
    user.map(|p| p.to_path_buf())
}

pub(crate) fn value_is_valid(setting: &Setting, value: &str) -> bool {
    match setting.kind {
        SettingKind::Duration => ff_core::snapshot::config::parse_keep(value).is_some(),
        SettingKind::Size => {
            ff_core::gix::config::Integer::try_from(ff_core::gix::bstr::BStr::new(value))
                .ok()
                .and_then(|i| i.to_decimal())
                .is_some_and(|n| n >= 0)
        }
        SettingKind::Command => !value.trim().is_empty(),
        SettingKind::Cadence => crate::cadence::parse(value).is_some(),
        SettingKind::Bool => {
            ff_core::gix::config::Boolean::try_from(ff_core::gix::bstr::BStr::new(value)).is_ok()
        }
    }
}

fn validate_value(setting: &Setting, value: &str) {
    if !value_is_valid(setting, value) {
        match setting.kind {
            SettingKind::Duration => {
                eprintln!(
                    "ff: invalid value for keep: want a duration like 90d, 36h, 2w, or days as a \
                     bare number"
                );
                std::process::exit(2);
            }
            SettingKind::Size => {
                eprintln!(
                    "ff: invalid value for maxFileSize: want a byte count like 52428800 or 100M"
                );
                std::process::exit(2);
            }
            SettingKind::Command => {
                eprintln!("ff: invalid value for pager: want a command");
                std::process::exit(2);
            }
            SettingKind::Cadence => {
                eprintln!(
                    "ff: invalid value for updateCheck: want true, false, or a duration like 12h or 7d"
                );
                std::process::exit(2);
            }
            SettingKind::Bool => {
                eprintln!("ff: invalid value for autoUpdate: want true or false");
                std::process::exit(2);
            }
        }
    }
}

fn dim(s: &str, colored: bool) -> String {
    if colored {
        let style = anstyle::Style::new().dimmed();
        format!("{style}{s}{style:#}")
    } else {
        s.to_string()
    }
}

fn scope_human_label(source: &str) -> &str {
    match source {
        "local" => "this repo",
        "global" => "global config",
        "system" => "system config",
        "env" => "the environment",
        _ => source,
    }
}

pub fn run(
    key: Option<String>,
    value: Option<String>,
    unset: bool,
    global: bool,
    json: bool,
) -> Result<()> {
    let repo = ff_core::discover(".")?;
    let colored = !matches!(
        anstream::AutoStream::choice(&std::io::stdout()),
        anstream::ColorChoice::Never
    );

    // No key: list all settings
    if key.is_none() && !unset {
        let snap = repo.config_snapshot();
        let file = snap.plumbing();

        let mut entries: Vec<serde_json::Value> = Vec::new();
        for setting in registry() {
            let effective = file.string(setting.key);
            let (val, source) = if effective.is_some() {
                scope_label_for(file, setting)
            } else {
                (None, "")
            };
            let is_default = val.is_none();
            let display = val.as_deref().unwrap_or(setting.def);

            if json {
                let kind_str = match setting.kind {
                    SettingKind::Size => "size",
                    SettingKind::Duration => "duration",
                    SettingKind::Command => "command",
                    SettingKind::Cadence => "cadence",
                    SettingKind::Bool => "bool",
                };
                let source_json = if source.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::json!(source)
                };
                let entry = serde_json::json!({
                    "key": setting.name,
                    "git_key": setting.key,
                    "kind": kind_str,
                    "value": display,
                    "source": source_json,
                    "default": is_default,
                    "description": setting.desc.join("\n"),
                });
                entries.push(entry);
            } else {
                let default_tag = if is_default {
                    format!(" {}", dim("(default)", colored))
                } else {
                    String::new()
                };
                println!("{}  {}{}", setting.name, display, default_tag);
                for line in setting.desc {
                    println!("  {}", line);
                }
                println!();
            }
        }

        if json {
            println!(
                "{}",
                serde_json::to_string(&serde_json::json!({"settings": entries}))
                    .map_err(Error::repo)?
            );
        }

        if !json {
            println!(
                "{}",
                dim(
                    "Set with:     ff config <key> <value>   (--global: every repo)",
                    colored
                )
            );
            println!("{}", dim("Remove with:  ff config --unset <key>", colored));
            println!(
                "{}",
                dim("Stored as plain git config under fufu.<key>", colored)
            );
        }
        return Ok(());
    }

    // Lookup the setting
    let input_key = key.as_deref().unwrap_or("");
    let setting = lookup_key(input_key);

    // Unset
    if unset {
        let snap = repo.config_snapshot();
        let file_snap = snap.plumbing();

        let removed_kind = if global {
            Kind::Global
        } else {
            Kind::Repository
        };

        let path = if global {
            match global_config_path() {
                Some(p) => p,
                None => {
                    return Err(Error::msg(
                        "cannot locate global git config: HOME is not set",
                    ));
                }
            }
        } else {
            repo.common_dir().join("config")
        };

        let source = if global { Source::User } else { Source::Local };

        let mut file = ff_core::snapshot::config::load_config_file(&path, source)?;

        let ids: Vec<_> = file
            .sections_and_ids_by_name("fufu")
            .into_iter()
            .flatten()
            .map(|(_, id)| id)
            .collect();
        let mut removed = false;
        for id in ids {
            if let Some(mut section) = file.section_mut_by_id(id) {
                while section.remove(setting.name).is_some() {
                    removed = true;
                }
            }
        }

        if !removed {
            // Not removed — check what still applies from other scopes
            let (still_val, still_source) = scope_label_excluding(file_snap, setting, removed_kind);
            if json {
                let still_json = still_val.as_ref().map(|v| {
                    serde_json::json!({
                        "value": v,
                        "source": still_source,
                    })
                });
                let body = serde_json::to_string(&serde_json::json!({
                    "key": setting.name,
                    "global": global,
                    "removed": false,
                    "still_applies": still_json,
                }))
                .map_err(Error::repo)?;
                println!("{body}");
            } else if still_val.is_some() {
                let suffix = if !global && still_source == "global" {
                    " — try --global"
                } else {
                    ""
                };
                println!(
                    "{} is not set here, but {} applies from {}{}",
                    setting.name,
                    still_val.unwrap(),
                    scope_human_label(still_source),
                    suffix
                );
            } else {
                println!(
                    "{} is not set — the default ({}) applies",
                    setting.name, setting.def
                );
            }
            return Ok(());
        }

        ff_core::snapshot::config::write_config_file(&path, &file)?;

        let (still_val, still_source) = scope_label_excluding(file_snap, setting, removed_kind);

        if setting.name == "updateCheck" {
            let encoded = still_val
                .as_deref()
                .and_then(crate::cadence::parse)
                .unwrap_or(0);
            crate::selfupdate::notify::sync_interval(encoded);
        }

        if setting.name == "autoTrim" {
            let encoded = still_val
                .as_deref()
                .and_then(crate::cadence::parse)
                .unwrap_or(0);
            crate::autotrim::sync_interval(&repo, encoded);
        }

        if json {
            let still_json = still_val.as_ref().map(|v| {
                serde_json::json!({
                    "value": v,
                    "source": still_source,
                })
            });
            let body = serde_json::to_string(&serde_json::json!({
                "key": setting.name,
                "global": global,
                "removed": true,
                "still_applies": still_json,
            }))
            .map_err(Error::repo)?;
            println!("{body}");
        } else if still_val.is_some() {
            println!(
                "{} unset here — {} still applies from {}",
                setting.name,
                still_val.unwrap(),
                scope_human_label(still_source)
            );
        } else {
            println!(
                "{} unset — back to the default ({})",
                setting.name, setting.def
            );
        }
        return Ok(());
    }

    // Get (key only, no value, no unset)
    if value.is_none() {
        let snap = repo.config_snapshot();
        let file = snap.plumbing();
        let effective = file.string(setting.key);

        let (val, source) = if effective.is_some() {
            scope_label_for(file, setting)
        } else {
            (None, "")
        };
        let is_default = val.is_none();
        let display = val.as_deref().unwrap_or(setting.def);

        if json {
            let source_json = if source.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::json!(source)
            };
            let body = serde_json::to_string(&serde_json::json!({
                "key": setting.name,
                "git_key": setting.key,
                "value": display,
                "source": source_json,
                "default": is_default,
            }))
            .map_err(Error::repo)?;
            println!("{body}");
        } else {
            println!("{display}");
        }
        return Ok(());
    }

    // Set (key + value)
    let new_value = value.unwrap();
    validate_value(setting, &new_value);

    let path = if global {
        match global_config_path() {
            Some(p) => p,
            None => {
                return Err(Error::msg(
                    "cannot locate global git config: HOME is not set",
                ));
            }
        }
    } else {
        repo.common_dir().join("config")
    };

    let source = if global { Source::User } else { Source::Local };

    let mut file = ff_core::snapshot::config::load_config_file(&path, source)?;
    file.set_raw_value_by("fufu", None, setting.name, new_value.as_str())
        .map_err(Error::repo)?;
    ff_core::snapshot::config::write_config_file(&path, &file)?;

    if setting.name == "updateCheck"
        && let Some(encoded) = crate::cadence::parse(&new_value)
    {
        crate::selfupdate::notify::sync_interval(encoded);
    }

    if setting.name == "autoTrim"
        && let Some(encoded) = crate::cadence::parse(&new_value)
    {
        crate::autotrim::sync_interval(&repo, encoded);
    }

    if json {
        let body = serde_json::to_string(&serde_json::json!({
            "key": setting.name,
            "value": new_value,
            "global": global,
        }))
        .map_err(Error::repo)?;
        println!("{body}");
    } else {
        let scope = if global { "every repo" } else { "this repo" };
        println!("{} = {} ({})", setting.name, new_value, scope);
    }

    Ok(())
}
