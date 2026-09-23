//! Profile overlay merge over base (global) policy documents.

use std::collections::BTreeMap;

use thiserror::Error;
use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, Value};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MergeError {
    #[error("failed to parse base policy: {0}")]
    BaseParse(String),
    #[error("failed to parse overlay: {0}")]
    OverlayParse(String),
}

pub fn merge_policy_documents(base: &str, overlay: &str) -> Result<String, MergeError> {
    let mut base_doc: DocumentMut = base
        .parse::<DocumentMut>()
        .map_err(|err| MergeError::BaseParse(err.to_string()))?;
    let overlay_doc: DocumentMut = overlay
        .parse::<DocumentMut>()
        .map_err(|err| MergeError::OverlayParse(err.to_string()))?;

    let mut remove_rules = collect_strings(&overlay_doc, "remove_rules");
    remove_rules.extend(collect_nested_strings(&overlay_doc, "remove_rules"));
    let mut remove_recognizers = collect_strings(&overlay_doc, "remove_recognizers");
    remove_recognizers.extend(collect_nested_strings(&overlay_doc, "remove_recognizers"));

    apply_rule_overrides(&mut base_doc, &overlay_doc, &remove_rules);
    apply_recognizer_overrides(&mut base_doc, &overlay_doc, &remove_recognizers);
    strip_overlay_keys(&mut base_doc);
    Ok(base_doc.to_string())
}

pub(crate) fn table_to_inline(table: &Table) -> InlineTable {
    let mut out = InlineTable::new();
    for (key, item) in table.iter() {
        match item {
            Item::Value(value) => {
                out.insert(key, value.clone());
            }
            Item::Table(nested) => {
                out.insert(key, Value::InlineTable(table_to_inline(nested)));
            }
            _ => {}
        }
    }
    out
}

pub(crate) fn item_to_values(item: Option<&Item>) -> Vec<Value> {
    match item {
        Some(Item::Value(Value::Array(array))) => array.iter().cloned().collect(),
        Some(Item::ArrayOfTables(aot)) => aot
            .iter()
            .map(|table| Value::InlineTable(table_to_inline(table)))
            .collect(),
        Some(Item::Table(table)) => vec![Value::InlineTable(table_to_inline(table))],
        _ => Vec::new(),
    }
}

fn collect_strings(doc: &DocumentMut, key: &str) -> Vec<String> {
    item_to_values(doc.get(key))
        .iter()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect()
}

fn collect_nested_strings(doc: &DocumentMut, key: &str) -> Vec<String> {
    doc.get("overrides")
        .and_then(Item::as_table_like)
        .and_then(|table| table.get(key))
        .map(|item| {
            item_to_values(Some(item))
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn nested_values(doc: &DocumentMut, table: &str, key: &str) -> Vec<Value> {
    doc.get(table)
        .and_then(Item::as_table_like)
        .map(|t| item_to_values(t.get(key)))
        .unwrap_or_default()
}

pub(crate) fn value_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .as_inline_table()
        .and_then(|t| t.get(key))
        .and_then(|v| v.as_str())
}

fn rule_identity_of(value: &Value) -> Option<String> {
    let table = value.as_inline_table()?;
    let kind = table.get("kind").and_then(|v| v.as_str())?;
    Some(match kind {
        "class" => format!(
            "class:{}",
            table
                .get("class")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
        ),
        "column" => format!(
            "column:{}",
            table
                .get("column")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
        ),
        "default" => "default".to_string(),
        other => format!("kind:{other}"),
    })
}

fn apply_rule_overrides(base: &mut DocumentMut, overlay: &DocumentMut, removes: &[String]) {
    let base_rules = item_to_values(base.get("rule"));
    let mut overrides: BTreeMap<String, Value> = BTreeMap::new();
    for value in nested_values(overlay, "overrides", "rules") {
        if let Some(identity) = rule_identity_of(&value) {
            overrides.insert(identity, value);
        }
    }
    for value in item_to_values(overlay.get("rule")) {
        if let Some(identity) = rule_identity_of(&value) {
            overrides.insert(identity, value);
        }
    }

    let mut retained: Vec<Value> = Vec::new();
    for value in base_rules {
        if let Some(identity) = rule_identity_of(&value) {
            if removes.iter().any(|r| r == &identity) {
                continue;
            }
            if overrides.contains_key(&identity) {
                continue;
            }
        }
        retained.push(value);
    }
    for (_, value) in overrides {
        retained.push(value);
    }

    let mut array = Array::new();
    for value in retained {
        array.push(value);
    }
    base.insert("rule", Item::Value(Value::Array(array)));
}

fn apply_recognizer_overrides(base: &mut DocumentMut, overlay: &DocumentMut, removes: &[String]) {
    let base_values = base
        .get("policy")
        .and_then(Item::as_table_like)
        .map(|policy| item_to_values(policy.get("custom_recognizers")))
        .unwrap_or_default();

    let mut overrides: BTreeMap<String, Value> = BTreeMap::new();
    for value in nested_values(overlay, "overrides", "recognizers") {
        if let Some(name) = value_str(&value, "name") {
            overrides.insert(name.to_string(), value);
        }
    }
    if let Some(policy) = overlay.get("policy").and_then(Item::as_table_like) {
        for value in item_to_values(policy.get("custom_recognizers")) {
            if let Some(name) = value_str(&value, "name") {
                overrides.insert(name.to_string(), value);
            }
        }
    }

    if base_values.is_empty() && removes.is_empty() && overrides.is_empty() {
        return;
    }

    let mut retained: Vec<Value> = Vec::new();
    for value in base_values {
        if let Some(name) = value_str(&value, "name") {
            if removes.iter().any(|r| r == name) {
                continue;
            }
            if overrides.contains_key(name) {
                continue;
            }
        }
        retained.push(value);
    }
    for (_, value) in overrides {
        retained.push(value);
    }

    if !base.contains_key("policy") {
        base.insert("policy", Item::Table(Table::new()));
    }
    let Some(policy) = base.get_mut("policy").and_then(Item::as_table_like_mut) else {
        return;
    };
    let mut array = Array::new();
    for value in retained {
        array.push(value);
    }
    policy.insert("custom_recognizers", Item::Value(Value::Array(array)));
}

fn strip_overlay_keys(doc: &mut DocumentMut) {
    doc.remove("remove_rules");
    doc.remove("remove_recognizers");
    doc.remove("overrides");
    if let Some(item) = doc.get_mut("schema_version") {
        *item = Item::Value(Value::from("0.1.0"));
    } else {
        doc.insert("schema_version", Item::Value(Value::from("0.1.0")));
    }
}
