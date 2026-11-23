//! Readable output from src/cli/human.ts and the session tables of
//! src/main/ssh/presentation.ts. Values get fields and tables, never a JSON dump.
use crate::human_text::{escape, table, text_width, wrap, Style};
use serde_json::{Map, Value};

fn label(key: &str, uppercase: bool) -> String {
    let mut text = String::with_capacity(key.len() + 4);
    let mut previous: Option<char> = None;
    for c in key.chars() {
        if c.is_ascii_uppercase() && previous.is_some_and(|p| p.is_ascii_lowercase()) {
            text.push(' ');
        }
        text.push(if c == '_' { ' ' } else { c });
        previous = Some(c);
    }
    escape(&if uppercase { text.to_uppercase() } else { text })
}
fn scalar(value: &Value) -> bool {
    !matches!(value, Value::Array(_) | Value::Object(_))
}
fn cell(value: &Value) -> String {
    match value {
        Value::Null => "none".into(),
        Value::Bool(true) => "yes".into(),
        Value::Bool(false) => "no".into(),
        Value::String(text) => escape(text),
        Value::Number(number) => number.to_string(),
        Value::Array(items) => items.len().to_string(),
        Value::Object(_) => String::new(),
    }
}
fn status(value: &str, style: Style) -> String {
    let lower = value.to_ascii_lowercase();
    match lower.as_str() {
        "error" | "failed" | "unavailable" | "closed" | "disabled" => style.red(value),
        "online" | "connected" | "open" | "running" | "yes" | "enabled" => style.green(value),
        _ => value.to_owned(),
    }
}
fn indent(text: &str) -> String {
    text.split('\n')
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}
fn has(value: &Map<String, Value>, keys: &[&str]) -> bool {
    keys.iter().all(|key| value.contains_key(*key))
}

fn status_table(headers: &[&str], rows: Vec<Vec<String>>, style: Style, width: usize) -> String {
    let headers: Vec<String> = headers.iter().map(|h| (*h).to_owned()).collect();
    table(&headers, &rows, style, width, &|value| status(value, style))
}

fn array_text(values: &[Value], style: Style, width: usize) -> String {
    if values.is_empty() {
        return "None.".into();
    }
    if values.iter().all(scalar) {
        return values.iter().map(cell).collect::<Vec<_>>().join("\n");
    }
    let records: Option<Vec<&Map<String, Value>>> = values.iter().map(Value::as_object).collect();
    if let Some(records) = records {
        let get =
            |record: &Map<String, Value>, key: &str| cell(record.get(key).unwrap_or(&Value::Null));
        if records.iter().all(|r| has(r, &["title", "cwd", "tabs"])) {
            let rows = records
                .iter()
                .map(|r| {
                    ["title", "id", "status", "tabs", "cwd"]
                        .map(|k| get(r, k))
                        .to_vec()
                })
                .collect();
            return status_table(
                &["NAME", "ID", "STATUS", "TERMINALS", "PATH"],
                rows,
                style,
                width,
            );
        }
        if records.iter().all(|r| has(r, &["sessionId", "lifecycle"])) {
            let keys = ["title", "id", "sessionId", "lifecycle", "profileId", "cwd"];
            let rows = records
                .iter()
                .map(|r| keys.map(|k| get(r, k)).to_vec())
                .collect();
            return status_table(
                &["NAME", "ID", "SESSION", "STATE", "PROFILE", "PATH"],
                rows,
                style,
                width,
            );
        }
        if records.iter().all(|r| has(r, &["hostname", "connection"])) {
            let rows = records
                .iter()
                .map(|r| {
                    let error = r
                        .get("lastError")
                        .and_then(Value::as_object)
                        .map_or_else(|| "none".into(), |e| get(e, "message"));
                    let mut row = ["hostname", "id", "online", "connection", "approved"]
                        .map(|k| get(r, k))
                        .to_vec();
                    row.push(error);
                    row
                })
                .collect();
            return status_table(
                &["MACHINE", "ID", "ONLINE", "CONNECTION", "APPROVED", "ERROR"],
                rows,
                style,
                width,
            );
        }
        let mut keys: Vec<&String> = Vec::new();
        for record in &records {
            for key in record.keys() {
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        keys.retain(|key| records.iter().all(|r| r.get(*key).is_none_or(scalar)));
        keys.truncate(8);
        if !keys.is_empty() {
            let headers: Vec<String> = keys.iter().map(|k| label(k, true)).collect();
            let rows = records
                .iter()
                .map(|r| keys.iter().map(|k| get(r, k)).collect())
                .collect::<Vec<_>>();
            let mut parts = vec![table(&headers, &rows, style, width, &|value| {
                status(value, style)
            })];
            for (index, record) in records.iter().enumerate() {
                let nested: Map<String, Value> = record
                    .iter()
                    .filter(|(_, item)| !scalar(item))
                    .map(|(key, item)| (key.clone(), item.clone()))
                    .collect();
                if !nested.is_empty() {
                    let name = record
                        .get("id")
                        .or_else(|| record.get("name"))
                        .map_or_else(|| (index + 1).to_string(), cell);
                    parts.push(format!(
                        "{}\n{}",
                        style.accent(&name),
                        indent(&render(
                            &Value::Object(nested),
                            style,
                            width.saturating_sub(2).max(1)
                        ))
                    ));
                }
            }
            return parts.join("\n");
        }
    }
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            format!(
                "{}\n{}",
                style.accent(&(index + 1).to_string()),
                indent(&render(value, style, width.saturating_sub(2).max(1)))
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 0.9 `renderHuman`.
pub fn render(value: &Value, style: Style, width: usize) -> String {
    let record = match value {
        Value::Array(values) => return wrap(&array_text(values, style, width), width),
        Value::Object(record) => record,
        value => return status(&wrap(&cell(value), width), style),
    };
    if record.is_empty() {
        return "Done.".into();
    }
    let fields: Vec<_> = record.iter().filter(|(_, item)| scalar(item)).collect();
    let size = fields
        .iter()
        .map(|(key, _)| text_width(&label(key, false)))
        .max()
        .unwrap_or(0);
    let mut lines: Vec<String> = fields
        .iter()
        .map(|(key, item)| {
            let name = label(key, false);
            let prefix = format!("{}  ", style.dim(&crate::human_text::pad(&name, size)));
            let value = status(&cell(item), style);
            if text_width(&prefix) + text_width(&value) <= width {
                format!("{prefix}{value}")
            } else {
                format!("{}\n{}", style.dim(&name), wrap(&value, width))
            }
        })
        .collect();
    for (key, item) in record.iter().filter(|(_, item)| !scalar(item)) {
        lines.push(format!(
            "{}\n{}",
            style.accent(&label(key, false)),
            indent(&render(item, style, width.saturating_sub(2).max(1)))
        ));
    }
    wrap(&lines.join("\n"), width)
}

#[cfg(test)]
#[path = "human_tests.rs"]
mod tests;
