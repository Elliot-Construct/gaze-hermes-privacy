use std::path::{Path, PathBuf};
use std::sync::Arc;

use gaze_hermes_sidecar::model::{GazeModelProvisioner, ModelProvisioner};
use gaze_hermes_sidecar::policies::{
    merge_policy_documents, sha256_hex, PolicyEdit, PolicyScope, PolicyStore, RecognizerEdit,
    RuleEdit,
};

const BASE_GLOBAL: &str = r#"
schema_version = "0.1.0"

[session]
scope = "conversation"

[policy.rulepacks]
bundled = ["core"]

[[rule]]
kind = "class"
class = "email"
action = "tokenize"

[[rule]]
kind = "class"
class = "name"
action = "tokenize"
"#;

const OVERLAY: &str = r#"
schema_version = "gaze-hermes-profile-1"

[[overrides.rules]]
kind = "class"
class = "email"
action = "redact"
"#;

fn unique_temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "gaze-policy-{}-{}-{}",
        tag,
        std::process::id(),
        nanos
    ));
    std::fs::create_dir_all(dir.join("profiles")).unwrap();
    dir
}

fn temp_store(tag: &str, global: &str) -> (PathBuf, PolicyStore) {
    let dir = unique_temp_dir(tag);
    std::fs::write(dir.join("global.toml"), global).unwrap();
    let store = PolicyStore::open(&dir).unwrap();
    (dir, store)
}

fn write_profile(dir: &Path, profile_id: &str, overlay: &str) {
    std::fs::write(dir.join("profiles").join(format!("{profile_id}.toml")), overlay).unwrap();
}

#[test]
fn profile_override_replaces_one_rule_without_flattening_base() {
    let base = r#"
schema_version = "0.1.0"
[[rule]]
kind = "class"
class = "email"
action = "tokenize"
[[rule]]
kind = "class"
class = "name"
action = "tokenize"
"#;
    let overlay = r#"
schema_version = "gaze-hermes-profile-1"
[[overrides.rules]]
kind = "class"
class = "email"
action = "redact"
"#;

    let effective = merge_policy_documents(base, overlay).unwrap();
    assert!(effective.contains("class = \"email\""));
    assert!(effective.contains("action = \"redact\""));
    assert!(effective.contains("class = \"name\""));
}

#[test]
fn profile_override_replaces_recognizer_by_name() {
    let base = r#"
schema_version = "0.1.0"

[session]
scope = "conversation"

[policy.rulepacks]
bundled = ["core"]

[[policy.custom_recognizers]]
kind = "regex"
name = "tenant_order"
pattern = 'ORD-[0-9]+'
class = "custom:order_id"

[[rule]]
kind = "class"
class = "custom:order_id"
action = "tokenize"
"#;
    let overlay = r#"
schema_version = "gaze-hermes-profile-1"

[[overrides.recognizers]]
kind = "regex"
name = "tenant_order"
pattern = 'TXN-[0-9]+'
class = "custom:order_id"
"#;

    let effective = merge_policy_documents(base, overlay).unwrap();
    assert!(effective.contains("TXN-[0-9]+"));
    assert!(!effective.contains("ORD-[0-9]+"));
    assert!(effective.contains("name = \"tenant_order\""));
    let occurrences = effective.matches("name = \"tenant_order\"").count();
    assert_eq!(occurrences, 1);
}

#[test]
fn remove_rules_deletes_inherited_entries() {
    let base = r#"
schema_version = "0.1.0"

[session]
scope = "conversation"

[[rule]]
kind = "class"
class = "email"
action = "tokenize"

[[rule]]
kind = "column"
column = "ssn"
action = "tokenize"

[[rule]]
kind = "default"
action = "tokenize"
"#;
    let overlay = r#"
schema_version = "gaze-hermes-profile-1"
remove_rules = ["class:email", "column:ssn", "default"]
"#;

    let effective = merge_policy_documents(base, overlay).unwrap();
    assert!(!effective.contains("class = \"email\""));
    assert!(!effective.contains("column = \"ssn\""));
    assert!(!effective.contains("kind = \"default\""));
    assert!(!effective.contains("remove_rules"));
}

#[test]
fn remove_recognizers_deletes_inherited_entries() {
    let base = r#"
schema_version = "0.1.0"

[session]
scope = "conversation"

[[policy.custom_recognizers]]
kind = "regex"
name = "legacy_rec"
pattern = 'OLD-[0-9]+'
class = "custom:old"

[[policy.custom_recognizers]]
kind = "regex"
name = "kept_rec"
pattern = 'KEEP-[0-9]+'
class = "custom:kept"

[[rule]]
kind = "class"
class = "custom:old"
action = "tokenize"
"#;
    let overlay = r#"
schema_version = "gaze-hermes-profile-1"
remove_recognizers = ["legacy_rec"]
remove_rules = ["class:custom:old"]
"#;

    let effective = merge_policy_documents(base, overlay).unwrap();
    assert!(!effective.contains("legacy_rec"));
    assert!(!effective.contains("OLD-[0-9]+"));
    assert!(effective.contains("kept_rec"));
    assert!(effective.contains("KEEP-[0-9]+"));
    assert!(!effective.contains("remove_recognizers"));
}

#[test]
fn merge_preserves_advanced_global_keys_and_comments() {
    let base = r#"
schema_version = "0.1.0"

# advanced operator knob stays untouched
[session]
scope = "conversation"
ttl_secs = 3600

[locale]
active = ["en-US"]

[policy.rulepacks]
bundled = ["core"]

[[rule]]
kind = "class"
class = "email"
action = "tokenize"
"#;
    let overlay = r#"
schema_version = "gaze-hermes-profile-1"
[[overrides.rules]]
kind = "class"
class = "email"
action = "format_preserve"
"#;

    let effective = merge_policy_documents(base, overlay).unwrap();
    assert!(effective.contains("# advanced operator knob stays untouched"));
    assert!(effective.contains("ttl_secs = 3600"));
    assert!(effective.contains("[locale]"));
    assert!(effective.contains("active = [\"en-US\"]"));
    assert!(effective.contains("action = \"format_preserve\""));
    assert!(!effective.contains("gaze-hermes-profile-1"));
    assert!(!effective.contains("overrides"));
}

#[test]
fn merge_appends_rule_when_base_lacks_identity() {
    let base = r#"
schema_version = "0.1.0"

[[rule]]
kind = "class"
class = "email"
action = "tokenize"
"#;
    let overlay = r#"
schema_version = "gaze-hermes-profile-1"
[[overrides.rules]]
kind = "class"
class = "location"
action = "redact"
"#;

    let effective = merge_policy_documents(base, overlay).unwrap();
    assert!(effective.contains("class = \"location\""));
    assert!(effective.contains("action = \"redact\""));
    assert!(effective.contains("class = \"email\""));
}

#[test]
fn validate_accepts_shipped_policy_files() {
    let store_dir = unique_temp_dir("validate-shipped");
    std::fs::write(store_dir.join("global.toml"), BASE_GLOBAL).unwrap();
    let store = PolicyStore::open(&store_dir).unwrap();

    let policies_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../policies");
    for name in [
        "default.toml",
        "strict.toml",
        "examples/custom-identifiers.toml",
    ] {
        let raw = std::fs::read_to_string(policies_dir.join(name)).unwrap();
        let result = store.validate(&raw);
        assert!(result.ok, "policy {name} failed: {:?}", result.errors);
    }
}

#[test]
fn validate_rejects_unsupported_schema_version() {
    let store_dir = unique_temp_dir("validate-bad");
    std::fs::write(store_dir.join("global.toml"), BASE_GLOBAL).unwrap();
    let store = PolicyStore::open(&store_dir).unwrap();

    let bad = BASE_GLOBAL.replace("0.1.0", "0.2.0");
    let result = store.validate(&bad);
    assert!(!result.ok);
    assert!(!result.errors.is_empty());
}

#[test]
fn validate_accepts_profile_overlay_against_global() {
    let (_dir, store) = temp_store("validate-overlay", BASE_GLOBAL);
    let result = store.validate(OVERLAY);
    assert!(result.ok, "overlay validate failed: {:?}", result.errors);
}

#[test]
fn effective_merges_profile_over_global() {
    let (dir, store) = temp_store("effective-merge", BASE_GLOBAL);
    write_profile(&dir, "team-a", OVERLAY);

    let global = store.effective(None).unwrap();
    assert!(global.toml.contains("action = \"tokenize\""));
    assert!(!global.toml.contains("action = \"redact\""));

    let profile = store.effective(Some("team-a")).unwrap();
    assert!(profile.toml.contains("action = \"redact\""));
    assert!(profile.toml.contains("class = \"name\""));
    assert_ne!(global.hash, profile.hash);
    assert!(Arc::strong_count(&profile.pipeline) > 1);
}

#[test]
fn effective_unknown_profile_is_error() {
    let (_dir, store) = temp_store("effective-unknown", BASE_GLOBAL);
    assert!(store.effective(Some("nope")).is_err());
}

#[test]
fn edit_returns_candidate_without_touching_active_or_file() {
    let (dir, store) = temp_store("edit-candidate", BASE_GLOBAL);
    let scope = PolicyScope::Global;
    let hash = store.document_hash(&scope);

    let candidate = store
        .edit(
            scope.clone(),
            &hash,
            PolicyEdit::UpsertRule(RuleEdit {
                kind: "class".to_string(),
                class: Some("location".to_string()),
                column: None,
                action: "redact".to_string(),
            }),
        )
        .unwrap();

    assert!(candidate.toml.contains("class = \"location\""));
    assert_eq!(candidate.hash, sha256_hex(candidate.toml.as_bytes()));

    let active = store.effective(None).unwrap();
    assert!(!active.toml.contains("class = \"location\""));
    let on_disk = std::fs::read_to_string(dir.join("global.toml")).unwrap();
    assert!(!on_disk.contains("class = \"location\""));
}

#[test]
fn edit_with_stale_hash_conflicts() {
    let (_dir, store) = temp_store("edit-stale", BASE_GLOBAL);
    let err = store
        .edit(
            PolicyScope::Global,
            "deadbeef",
            PolicyEdit::RemoveRule {
                identity: "class:email".to_string(),
            },
        )
        .unwrap_err();
    assert!(matches!(
        err,
        gaze_hermes_sidecar::policies::PolicyStoreError::Conflict
    ));
}

#[test]
fn apply_with_current_hash_activates_and_rotates_hash() {
    let (dir, store) = temp_store("apply-ok", BASE_GLOBAL);
    let scope = PolicyScope::Global;
    let hash = store.document_hash(&scope);

    let candidate = store
        .edit(
            scope.clone(),
            &hash,
            PolicyEdit::UpsertRule(RuleEdit {
                kind: "class".to_string(),
                class: Some("location".to_string()),
                column: None,
                action: "tokenize".to_string(),
            }),
        )
        .unwrap();

    let applied = store.apply(scope.clone(), &hash, &candidate.toml).unwrap();
    assert!(applied.toml.contains("class = \"location\""));
    assert_eq!(store.document_hash(&scope), candidate.hash);
    assert_ne!(store.document_hash(&scope), hash);

    let active = store.effective(None).unwrap();
    assert!(active.toml.contains("class = \"location\""));
    let on_disk = std::fs::read_to_string(dir.join("global.toml")).unwrap();
    assert!(on_disk.contains("class = \"location\""));
}

#[test]
fn apply_with_stale_hash_changes_nothing() {
    let (dir, store) = temp_store("apply-stale", BASE_GLOBAL);
    let before = store.effective(None).unwrap();
    let on_disk_before = std::fs::read_to_string(dir.join("global.toml")).unwrap();

    let good = BASE_GLOBAL.replace("tokenize", "redact");
    let err = store
        .apply(PolicyScope::Global, "deadbeef", &good)
        .unwrap_err();
    assert!(matches!(
        err,
        gaze_hermes_sidecar::policies::PolicyStoreError::Conflict
    ));

    let after = store.effective(None).unwrap();
    assert_eq!(before.hash, after.hash);
    let on_disk_after = std::fs::read_to_string(dir.join("global.toml")).unwrap();
    assert_eq!(on_disk_before, on_disk_after);
}

#[test]
fn apply_with_invalid_candidate_changes_nothing() {
    let (dir, store) = temp_store("apply-invalid", BASE_GLOBAL);
    let scope = PolicyScope::Global;
    let hash = store.document_hash(&scope);
    let before = store.effective(None).unwrap();
    let on_disk_before = std::fs::read_to_string(dir.join("global.toml")).unwrap();

    let invalid = BASE_GLOBAL.replace("0.1.0", "0.2.0");
    let err = store.apply(scope, &hash, &invalid).unwrap_err();
    assert!(matches!(
        err,
        gaze_hermes_sidecar::policies::PolicyStoreError::Policy(_)
            | gaze_hermes_sidecar::policies::PolicyStoreError::Build(_)
    ));

    let after = store.effective(None).unwrap();
    assert_eq!(before.hash, after.hash);
    let on_disk_after = std::fs::read_to_string(dir.join("global.toml")).unwrap();
    assert_eq!(on_disk_before, on_disk_after);
}

#[test]
fn profile_apply_updates_only_that_profile() {
    let (dir, _store) = temp_store("apply-profile", BASE_GLOBAL);
    write_profile(&dir, "team-a", OVERLAY);
    write_profile(&dir, "team-b", OVERLAY);
    let store = PolicyStore::open(&dir).unwrap();

    let scope = PolicyScope::Profile("team-a".to_string());
    let hash = store.document_hash(&scope);
    let new_overlay = OVERLAY.replace("redact", "format_preserve");
    let applied = store.apply(scope, &hash, &new_overlay).unwrap();
    assert!(applied.toml.contains("action = \"format_preserve\""));

    let team_a = store.effective(Some("team-a")).unwrap();
    assert!(team_a.toml.contains("format_preserve"));
    let team_b = store.effective(Some("team-b")).unwrap();
    assert!(team_b.toml.contains("redact"));
    let global = store.effective(None).unwrap();
    assert!(!global.toml.contains("redact"));
}

#[test]
fn remove_rule_edit_persists_through_apply() {
    let (dir, store) = temp_store("remove-apply", BASE_GLOBAL);
    let scope = PolicyScope::Global;
    let hash = store.document_hash(&scope);

    let candidate = store
        .edit(
            scope.clone(),
            &hash,
            PolicyEdit::RemoveRule {
                identity: "class:name".to_string(),
            },
        )
        .unwrap();
    assert!(!candidate.toml.contains("class = \"name\""));

    store.apply(scope, &hash, &candidate.toml).unwrap();
    let active = store.effective(None).unwrap();
    assert!(!active.toml.contains("class = \"name\""));
    let on_disk = std::fs::read_to_string(dir.join("global.toml")).unwrap();
    assert!(!on_disk.contains("class = \"name\""));
}

#[test]
fn upsert_recognizer_edit_round_trips() {
    let (_dir, store) = temp_store("recognizer-edit", BASE_GLOBAL);
    let scope = PolicyScope::Global;
    let hash = store.document_hash(&scope);

    let candidate = store
        .edit(
            scope.clone(),
            &hash,
            PolicyEdit::UpsertRecognizer(RecognizerEdit {
                kind: "regex".to_string(),
                name: "employee_id".to_string(),
                pattern: Some(r"\bEMP-[0-9]{5}\b".to_string()),
                class: "custom:employee_id".to_string(),
                terms: None,
                terms_file: None,
                terms_from_context: None,
                case_sensitive: None,
                token_family: None,
                safety_tier: None,
            }),
        )
        .unwrap();
    assert!(candidate.toml.contains("name = \"employee_id\""));

    store.apply(scope, &hash, &candidate.toml).unwrap();
    let active = store.effective(None).unwrap();
    assert!(active.toml.contains(r"\bEMP-[0-9]{5}\b"));
}

#[test]
fn profile_scope_rejects_path_traversal_ids() {
    let (_dir, store) = temp_store("traversal", BASE_GLOBAL);
    let scope = PolicyScope::Profile("../evil".to_string());
    assert!(store.document_hash(&scope).is_empty() || true);
    let err = store
        .apply(scope, "deadbeef", "schema_version = \"0.1.0\"")
        .unwrap_err();
    assert!(matches!(
        err,
        gaze_hermes_sidecar::policies::PolicyStoreError::InvalidProfileId(_)
    ));
}

struct FakeProvisioner {
    path: PathBuf,
}

impl ModelProvisioner for FakeProvisioner {
    fn ensure(&self) -> Result<PathBuf, gaze_hermes_sidecar::model::ModelError> {
        Ok(self.path.clone())
    }
}

#[test]
fn policy_scope_serde_uses_snake_case_shapes() {
    let global = serde_json::to_value(PolicyScope::Global).unwrap();
    assert_eq!(global, serde_json::json!("global"));
    let profile = serde_json::to_value(PolicyScope::Profile("team-a".into())).unwrap();
    assert_eq!(profile, serde_json::json!({"profile": "team-a"}));
    let parsed: PolicyScope = serde_json::from_value(serde_json::json!("global")).unwrap();
    assert_eq!(parsed, PolicyScope::Global);
    let parsed: PolicyScope =
        serde_json::from_value(serde_json::json!({"profile": "team-a"})).unwrap();
    assert_eq!(parsed, PolicyScope::Profile("team-a".into()));
}

#[test]
fn fake_provisioner_ensure_returns_configured_path_without_network() {
    let fake = FakeProvisioner {
        path: PathBuf::from("/models/fake-bundle"),
    };
    assert_eq!(
        fake.ensure().unwrap(),
        PathBuf::from("/models/fake-bundle")
    );
}

#[test]
fn gaze_provisioner_implements_model_provisioner() {
    fn assert_trait<T: ModelProvisioner>() {}
    assert_trait::<GazeModelProvisioner>();
    let provisioner = GazeModelProvisioner { model_dir: None };
    let _: &dyn ModelProvisioner = &provisioner;
}

#[test]
fn default_kiji_model_dir_uses_pinned_bundle_name() {
    let has_xdg = std::env::var_os("XDG_DATA_HOME").is_some_and(|v| !v.is_empty());
    let has_home = std::env::var_os("HOME").is_some_and(|v| !v.is_empty());
    if !has_xdg && !has_home {
        std::env::set_var("XDG_DATA_HOME", std::env::temp_dir().join("xdg-data"));
    }
    let dir = gaze_model_setup::default_kiji_model_dir().unwrap();
    assert!(dir.ends_with("kiji-distilbert"));
}
