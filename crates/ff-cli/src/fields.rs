//! `--fields`: the projection of a `--json` payload to dotted paths.
//!
//! The flag is the general answer to "I want two keys per row" that
//! `docs/agents/machine-surface.md` used to give with jq. It is only a
//! projection: no template language, no renaming, no computed values. The
//! envelope already does jj's template job, so the value is shaped and
//! nothing else.

use std::collections::BTreeMap;

use ff_core::{Error, Result};
use serde_json::Value;

/// The dotted paths `--fields` keeps of a payload, as a trie: `commits.id`
/// and `commits.subject` share the `commits` node; a node named whole
/// (`commits`) keeps everything under it, so `commits,commits.id` is `commits`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fields {
    children: BTreeMap<String, Node>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Node {
    whole: bool,
    children: BTreeMap<String, Node>,
}

impl Fields {
    /// `--fields`'s value: comma-separated dotted paths, whitespace around
    /// each trimmed. An empty list, an empty path, or an empty segment
    /// (`a..b`, `.a`, `a.`) is `usage/bad-flags`.
    pub fn parse(raw: &str) -> Result<Fields> {
        if raw.trim().is_empty() {
            return Err(Error::coded(
                "usage/bad-flags",
                "--fields needs at least one dotted path, such as commits.subject",
                vec![],
            ));
        }
        let mut fields = Fields::default();
        for path in raw.split(',') {
            let path = path.trim();
            if path.is_empty() {
                return Err(Error::coded(
                    "usage/bad-flags",
                    format!("--fields {raw}: an empty path in the list"),
                    vec![],
                ));
            }
            let mut children = &mut fields.children;
            let mut segments = path.split('.').peekable();
            while let Some(segment) = segments.next() {
                if segment.is_empty() {
                    return Err(Error::coded(
                        "usage/bad-flags",
                        format!("--fields {path}: an empty segment; a path is dotted keys"),
                        vec![],
                    ));
                }
                let node = children.entry(segment.to_string()).or_default();
                if segments.peek().is_none() {
                    node.whole = true;
                }
                children = &mut node.children;
            }
        }
        Ok(fields)
    }

    /// `data` cut down to the paths. Objects keep the named keys in payload
    /// order, each projected on; an array applies the node to every element;
    /// a path that reaches no key is `usage/no-such-field`.
    pub fn project(&self, data: Value) -> Result<Value> {
        let mut path = Vec::new();
        walk(&self.children, data, &mut path)
    }
}

/// One node's children applied to one value, with the path so far for the
/// message. An array adds nothing to the path: the message names the key
/// whose rows lacked something, not which row.
fn walk(children: &BTreeMap<String, Node>, value: Value, path: &mut Vec<String>) -> Result<Value> {
    match value {
        Value::Object(map) => {
            if let Some(key) = children.keys().find(|key| !map.contains_key(*key)) {
                return Err(missing(path, key, map.keys()));
            }
            let mut out = serde_json::Map::new();
            for (key, val) in map {
                let Some(node) = children.get(&key) else {
                    continue;
                };
                let kept = if node.whole {
                    val
                } else {
                    path.push(key.clone());
                    let kept = walk(&node.children, val, path)?;
                    path.pop();
                    kept
                };
                out.insert(key, kept);
            }
            Ok(Value::Object(out))
        }
        Value::Array(items) => items
            .into_iter()
            .map(|item| walk(children, item, path))
            .collect::<Result<Vec<_>>>()
            .map(Value::Array),
        other => Err(wrong_kind(path, children, &other)),
    }
}

/// The refusal for a key the object at `path` does not carry. Every key the
/// object does carry is named, in payload order, so the answer is on the line.
fn missing<'a>(path: &[String], key: &str, keys: impl Iterator<Item = &'a String>) -> Error {
    let keys: Vec<&str> = keys.map(String::as_str).collect();
    let available = if keys.is_empty() {
        "it has no keys".to_string()
    } else {
        format!("its keys are {}", keys.join(", "))
    };
    Error::coded(
        "usage/no-such-field",
        format!(
            "--fields {} matches nothing: {} has no `{key}`; {available}",
            dotted(path, key),
            whose(path),
        ),
        vec![],
    )
}

/// The refusal for a path that continues below a null or a scalar.
fn wrong_kind(path: &[String], children: &BTreeMap<String, Node>, value: &Value) -> Error {
    let kind = match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) | Value::Object(_) => unreachable!("arrays and objects are walked"),
    };
    let key = children
        .keys()
        .next()
        .map(String::as_str)
        .unwrap_or_default();
    Error::coded(
        "usage/no-such-field",
        format!(
            "--fields {} matches nothing: {} is {kind}",
            dotted(path, key),
            whose(path),
        ),
        vec![],
    )
}

/// The path as the user would spell it: the segments so far, then `key`.
fn dotted(path: &[String], key: &str) -> String {
    path.iter()
        .map(String::as_str)
        .chain(std::iter::once(key))
        .collect::<Vec<_>>()
        .join(".")
}

/// What the message calls the value at `path`: its last key, or the payload
/// itself at the root.
fn whose(path: &[String]) -> String {
    match path.last() {
        Some(key) => format!("`{key}`"),
        None => "the payload".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fields(raw: &str) -> Fields {
        Fields::parse(raw).expect("a well-formed list")
    }

    fn refusal(raw: &str, data: Value) -> Error {
        fields(raw).project(data).expect_err("a refusal")
    }

    #[test]
    fn parse_trims_each_path() {
        assert_eq!(fields(" a.b , c "), fields("a.b,c"));
    }

    #[test]
    fn parse_refuses_an_empty_list() {
        for raw in ["", "  "] {
            let err = Fields::parse(raw).expect_err("refused");
            assert_eq!(err.id(), "usage/bad-flags");
            assert!(
                err.to_string().contains("at least one dotted path"),
                "{err}"
            );
        }
    }

    #[test]
    fn parse_refuses_an_empty_path_or_segment() {
        for raw in ["a,,b", "a,", "a..b", ".a", "a."] {
            let err = Fields::parse(raw).expect_err(raw);
            assert_eq!(err.id(), "usage/bad-flags", "{raw}");
            assert!(err.to_string().starts_with("--fields "), "{raw}: {err}");
        }
    }

    #[test]
    fn a_whole_node_keeps_everything_under_it() {
        let data = json!({"open": {"id": 1, "subject": "s", "body": "b"}, "n": 2});
        let kept = fields("open").project(data.clone()).unwrap();
        assert_eq!(
            kept,
            json!({"open": {"id": 1, "subject": "s", "body": "b"}})
        );
        // Named whole and by a child: whole wins.
        let kept = fields("open.id,open").project(data).unwrap();
        assert_eq!(
            kept,
            json!({"open": {"id": 1, "subject": "s", "body": "b"}})
        );
    }

    #[test]
    fn a_nested_path_keeps_its_nesting_and_the_payload_order() {
        let data = json!({"open": {"id": 1, "subject": "s", "pending": true}, "n": 2});
        let kept = fields("open.pending,open.id").project(data).unwrap();
        assert_eq!(kept, json!({"open": {"id": 1, "pending": true}}));
        let text = serde_json::to_string(&kept).unwrap();
        assert_eq!(text, r#"{"open":{"id":1,"pending":true}}"#);
    }

    #[test]
    fn an_array_maps_the_node_over_every_element() {
        let data = json!({"rows": [{"id": 1, "x": 0}, {"id": 2, "x": 0}]});
        let kept = fields("rows.id").project(data).unwrap();
        assert_eq!(kept, json!({"rows": [{"id": 1}, {"id": 2}]}));
    }

    #[test]
    fn an_empty_array_passes() {
        let kept = fields("rows.id").project(json!({"rows": []})).unwrap();
        assert_eq!(kept, json!({"rows": []}));
    }

    #[test]
    fn a_missing_key_names_the_keys_there() {
        let data = json!({"commits": [{"id": 1, "subject": "s", "body": "b"}]});
        let err = refusal("commits.subjet", data);
        assert_eq!(err.id(), "usage/no-such-field");
        assert_eq!(
            err.to_string(),
            "--fields commits.subjet matches nothing: `commits` has no `subjet`; \
             its keys are id, subject, body"
        );
        let err = refusal("subjet", json!({"subject": "s"}));
        assert_eq!(
            err.to_string(),
            "--fields subjet matches nothing: the payload has no `subjet`; its keys are subject"
        );
    }

    #[test]
    fn a_path_below_a_scalar_names_its_kind() {
        let err = refusal("open.subject", json!({"open": null}));
        assert_eq!(err.id(), "usage/no-such-field");
        assert_eq!(
            err.to_string(),
            "--fields open.subject matches nothing: `open` is null"
        );
        let err = refusal("head.name", json!({"head": "main"}));
        assert_eq!(
            err.to_string(),
            "--fields head.name matches nothing: `head` is a string"
        );
    }
}
