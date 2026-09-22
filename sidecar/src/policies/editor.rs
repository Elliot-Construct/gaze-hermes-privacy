//! Visual-editor patch types for policy documents.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyScope {
    Global,
    Profile(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuleEdit {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecognizerEdit {
    pub kind: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    pub class: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terms: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terms_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terms_from_context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub case_sensitive: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_tier: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyEdit {
    UpsertRule(RuleEdit),
    RemoveRule { identity: String },
    UpsertRecognizer(RecognizerEdit),
    RemoveRecognizer { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditedPolicy {
    pub toml: String,
    pub hash: String,
}

pub fn rule_identity(kind: &str, class: Option<&str>, column: Option<&str>) -> String {
    match kind {
        "class" => format!(
            "class:{}",
            class.unwrap_or_default()
        ),
        "column" => format!("column:{}", column.unwrap_or_default()),
        "default" => "default".to_string(),
        other => format!("kind:{other}"),
    }
}

pub(crate) fn is_profile_overlay(raw: &str) -> bool {
    raw.contains("schema_version = \"gaze-hermes-profile-1\"")
        || raw.contains("[overrides")
        || raw.contains("remove_rules")
        || raw.contains("remove_recognizers")
}
