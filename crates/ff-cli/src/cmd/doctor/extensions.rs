use super::Row;

/// Every `ff-<name>` on PATH, as one aggregate row.
///
/// A binary on PATH is fufu's shell surface working as designed — `ff
/// <name>` runs it git-style — so it earns an `info` row at most and never
/// a `WARN`. There is nothing to check against: fufu records nothing about
/// an extension, so the row says what the PATH walk found and no more.
pub(super) fn extension_rows() -> Vec<Row> {
    let found = crate::ext::on_path();
    if found.is_empty() {
        return Vec::new();
    }
    let named: Vec<String> = found.iter().map(|name| format!("ff-{name}")).collect();
    vec![Row::info(
        "extensions",
        format!("{} on PATH: {}", found.len(), named.join(", ")),
    )]
}
