//! Policy store: global + profile overlays, validation, optimistic CAS apply.

pub mod editor;
pub mod merge;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};
use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, Value};

pub use editor::{rule_identity, EditedPolicy, PolicyEdit, PolicyScope, RecognizerEdit, RuleEdit};
pub use merge::{merge_policy_documents, MergeError};

#[derive(Debug, thiserror::Error)]
pub enum PolicyStoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("policy parse/load error: {0}")]
    Policy(String),
    #[error("pipeline build error: {0}")]
    Build(String),
    #[error("merge error: {0}")]
    Merge(#[from] MergeError),
    #[error("hash conflict")]
    Conflict,
    #[error("unknown profile: {0}")]
    UnknownProfile(String),
    #[error("invalid profile id: {0}")]
    InvalidProfileId(String),
    #[error("unknown bundled rulepack: {0}")]
    UnknownBundledRulepack(String),
}

#[derive(Debug, Clone)]
pub struct EffectivePolicy {
    pub toml: String,
    pub hash: String,
    pub policy: gaze::Policy,
    pub pipeline: Arc<gaze::Pipeline>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ValidationResult {
    pub ok: bool,
    pub errors: Vec<String>,
}

impl ValidationResult {
    pub fn ok() -> Self {
        Self {
            ok: true,
            errors: Vec::new(),
        }
    }

    pub fn from_error(err: impl std::fmt::Display) -> Self {
        Self {
            ok: false,
            errors: vec![err.to_string()],
        }
    }
}

struct StoreData {
    global: String,
    profiles: BTreeMap<String, String>,
    cache: BTreeMap<Option<String>, EffectivePolicy>,
}

pub struct PolicyStore {
    dir: PathBuf,
    data: Mutex<StoreData>,
}

impl PolicyStore {
    pub fn open(dir: &Path) -> Result<Self, PolicyStoreError> {
        let global = std::fs::read_to_string(dir.join("global.toml"))?;
        let mut profiles = BTreeMap::new();
        let profiles_dir = dir.join("profiles");
        if profiles_dir.is_dir() {
            for entry in std::fs::read_dir(&profiles_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        let id = stem.to_string();
                        validate_profile_id(&id)?;
                        profiles.insert(id, std::fs::read_to_string(&path)?);
                    }
                }
            }
        }

        let store = Self {
            dir: dir.to_path_buf(),
            data: Mutex::new(StoreData {
                global: global.clone(),
                profiles,
                cache: BTreeMap::new(),
            }),
        };

        let global_eff = store.build_effective(&global)?;
        store.data.lock().unwrap().cache.insert(None, global_eff);

        let ids: Vec<String> = store
            .data
            .lock()
            .unwrap()
            .profiles
            .keys()
            .cloned()
            .collect();
        for id in ids {
            let overlay = store.data.lock().unwrap().profiles[&id].clone();
            let merged = merge_policy_documents(&global, &overlay)?;
            let eff = store.build_effective(&merged)?;
            store.data.lock().unwrap().cache.insert(Some(id), eff);
        }

        Ok(store)
    }

    pub fn effective(&self, profile_id: Option<&str>) -> Result<EffectivePolicy, PolicyStoreError> {
        if let Some(id) = profile_id {
            validate_profile_id(id)?;
        }

        if let Some(eff) = self
            .data
            .lock()
            .unwrap()
            .cache
            .get(&profile_id.map(str::to_string))
            .cloned()
        {
            return Ok(eff);
        }

        if let Some(id) = profile_id {
            let overlay_path = self.dir.join("profiles").join(format!("{id}.toml"));
            if overlay_path.exists() {
                let overlay = std::fs::read_to_string(&overlay_path)?;
                let global = self.data.lock().unwrap().global.clone();
                let merged = merge_policy_documents(&global, &overlay)?;
                let eff = self.build_effective(&merged)?;
                let mut data = self.data.lock().unwrap();
                data.profiles.insert(id.to_string(), overlay);
                data.cache.insert(Some(id.to_string()), eff.clone());
                return Ok(eff);
            }
            return Err(PolicyStoreError::UnknownProfile(id.to_string()));
        }

        Err(PolicyStoreError::UnknownProfile(String::new()))
    }

    pub fn document_hash(&self, scope: &PolicyScope) -> String {
        match scope {
            PolicyScope::Global => sha256_hex(self.data.lock().unwrap().global.as_bytes()),
            PolicyScope::Profile(id) => {
                if validate_profile_id(id).is_err() {
                    return String::new();
                }
                let data = self.data.lock().unwrap();
                match data.profiles.get(id) {
                    Some(doc) => sha256_hex(doc.as_bytes()),
                    None => String::new(),
                }
            }
        }
    }

    pub fn validate(&self, raw_toml: &str) -> ValidationResult {
        let candidate = if is_overlay_document(raw_toml) {
            let global = self.data.lock().unwrap().global.clone();
            match merge_policy_documents(&global, raw_toml) {
                Ok(merged) => merged,
                Err(err) => {
                    return ValidationResult::from_error(err);
                }
            }
        } else {
            raw_toml.to_string()
        };

        match self.try_build(&candidate) {
            Ok(_) => ValidationResult::ok(),
            Err(err) => ValidationResult::from_error(err),
        }
    }

    pub fn edit(
        &self,
        scope: PolicyScope,
        expected_hash: &str,
        edit: PolicyEdit,
    ) -> Result<EditedPolicy, PolicyStoreError> {
        self.check_hash(&scope, expected_hash)?;
        let current = self.document_for(&scope)?;
        let candidate = apply_edit_to_document(&current, &edit, &scope, self)?;
        Ok(EditedPolicy {
            hash: sha256_hex(candidate.as_bytes()),
            toml: candidate,
        })
    }

    pub fn apply(
        &self,
        scope: PolicyScope,
        expected_hash: &str,
        raw_toml: &str,
    ) -> Result<EffectivePolicy, PolicyStoreError> {
        self.check_hash(&scope, expected_hash)?;

        let primary_eff = match &scope {
            PolicyScope::Global => self.try_build(raw_toml)?,
            PolicyScope::Profile(id) => {
                validate_profile_id(id)?;
                let global = self.data.lock().unwrap().global.clone();
                let merged = merge_policy_documents(&global, raw_toml)?;
                self.try_build(&merged)?
            }
        };

        let profile_rebuilds: Result<Vec<(String, EffectivePolicy)>, PolicyStoreError> =
            match &scope {
                PolicyScope::Global => {
                    let overlays: Vec<(String, String)> = {
                        let data = self.data.lock().unwrap();
                        data.profiles
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect()
                    };
                    let mut out = Vec::new();
                    for (id, overlay) in overlays {
                        let merged = merge_policy_documents(raw_toml, &overlay)?;
                        let eff = self.try_build(&merged)?;
                        out.push((id, eff));
                    }
                    Ok(out)
                }
                PolicyScope::Profile(_) => Ok(Vec::new()),
            };
        let profile_rebuilds = profile_rebuilds?;

        atomic_write(&self.path_for(&scope), raw_toml.as_bytes())?;

        {
            let mut data = self.data.lock().unwrap();
            match &scope {
                PolicyScope::Global => {
                    data.global = raw_toml.to_string();
                    data.cache.insert(None, primary_eff.clone());
                    for (id, eff) in profile_rebuilds {
                        data.cache.insert(Some(id), eff);
                    }
                }
                PolicyScope::Profile(id) => {
                    data.profiles.insert(id.clone(), raw_toml.to_string());
                    data.cache.insert(Some(id.clone()), primary_eff.clone());
                }
            }
        }

        Ok(primary_eff)
    }

    fn check_hash(&self, scope: &PolicyScope, expected_hash: &str) -> Result<(), PolicyStoreError> {
        match scope {
            PolicyScope::Profile(id) => validate_profile_id(id)?,
            PolicyScope::Global => {}
        }
        if self.document_hash(scope) != expected_hash {
            return Err(PolicyStoreError::Conflict);
        }
        Ok(())
    }

    fn document_for(&self, scope: &PolicyScope) -> Result<String, PolicyStoreError> {
        let data = self.data.lock().unwrap();
        match scope {
            PolicyScope::Global => Ok(data.global.clone()),
            PolicyScope::Profile(id) => data
                .profiles
                .get(id)
                .cloned()
                .ok_or_else(|| PolicyStoreError::UnknownProfile(id.clone())),
        }
    }

    fn path_for(&self, scope: &PolicyScope) -> PathBuf {
        match scope {
            PolicyScope::Global => self.dir.join("global.toml"),
            PolicyScope::Profile(id) => self.dir.join("profiles").join(format!("{id}.toml")),
        }
    }

    fn build_effective(&self, effective_toml: &str) -> Result<EffectivePolicy, PolicyStoreError> {
        self.try_build(effective_toml)
    }

    fn try_build(&self, effective_toml: &str) -> Result<EffectivePolicy, PolicyStoreError> {
        let (policy, pipeline) = self.try_build_inner(effective_toml)?;
        Ok(EffectivePolicy {
            hash: sha256_hex(effective_toml.as_bytes()),
            toml: effective_toml.to_string(),
            policy,
            pipeline,
        })
    }

    fn try_build_inner(
        &self,
        effective_toml: &str,
    ) -> Result<(gaze::Policy, Arc<gaze::Pipeline>), PolicyStoreError> {
        let staging = self.dir.join(format!(
            ".staging-candidate-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(&staging, effective_toml)?;
        let result = load_and_build(&staging, &self.dir);
        let _ = std::fs::remove_file(&staging);
        result
    }
}

fn load_and_build(
    path: &Path,
    store_dir: &Path,
) -> Result<(gaze::Policy, Arc<gaze::Pipeline>), PolicyStoreError> {
    let policy =
        gaze::Policy::load(path).map_err(|err| PolicyStoreError::Policy(err.to_string()))?;

    let mut rulepacks = Vec::new();
    for name in policy.rulepacks.bundled.clone() {
        match name.as_str() {
            "core" | "core-extended" => {
                let embedded = gaze_recognizers::embedded(&name)
                    .or_else(|| gaze_recognizers::embedded("core"))
                    .ok_or_else(|| PolicyStoreError::UnknownBundledRulepack(name.clone()))?;
                let pack = gaze::Rulepack::load(gaze::RulepackSource::Embedded(embedded))
                    .map_err(|err| PolicyStoreError::Policy(err.to_string()))?;
                rulepacks.push(pack);
            }
            other => {
                let candidate = store_dir.join(other);
                if candidate.exists() {
                    let pack = gaze::Rulepack::load(gaze::RulepackSource::Path(candidate))
                        .map_err(|err| PolicyStoreError::Policy(err.to_string()))?;
                    rulepacks.push(pack);
                } else {
                    return Err(PolicyStoreError::UnknownBundledRulepack(other.to_string()));
                }
            }
        }
    }

    for path_entry in policy.rulepacks.paths.clone() {
        let full = if path_entry.is_absolute() {
            path_entry
        } else {
            store_dir.join(path_entry)
        };
        let pack = gaze::Rulepack::load(gaze::RulepackSource::Path(full))
            .map_err(|err| PolicyStoreError::Policy(err.to_string()))?;
        rulepacks.push(pack);
    }

    let locale_chain = gaze::LocaleChain::from_tags(policy.locale.clone().unwrap_or_default());

    let context = gaze::Context {
        dictionaries: std::collections::HashMap::new(),
        class_map: std::collections::HashMap::new(),
        fields: Default::default(),
    };

    let pipeline =
        gaze_assembly::build_pipeline(&policy, &context, &rulepacks, &locale_chain, None)
            .map_err(|err| PolicyStoreError::Build(err.to_string()))?;

    Ok((policy, Arc::new(pipeline)))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn validate_profile_id(id: &str) -> Result<(), PolicyStoreError> {
    if id.is_empty()
        || id.contains('/')
        || id.contains('\\')
        || id.contains("..")
        || id.starts_with('.')
    {
        return Err(PolicyStoreError::InvalidProfileId(id.to_string()));
    }
    Ok(())
}

pub fn validate_profile_id_public(id: &str) -> Result<(), PolicyStoreError> {
    validate_profile_id(id)
}

fn is_overlay_document(raw: &str) -> bool {
    editor::is_profile_overlay(raw)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), PolicyStoreError> {
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn apply_edit_to_document(
    current: &str,
    edit: &PolicyEdit,
    scope: &PolicyScope,
    store: &PolicyStore,
) -> Result<String, PolicyStoreError> {
    match scope {
        PolicyScope::Global => apply_global_edit(current, edit),
        PolicyScope::Profile(_) => apply_profile_edit(current, edit, store),
    }
}

fn parse_doc(raw: &str) -> Result<DocumentMut, PolicyStoreError> {
    raw.parse::<DocumentMut>()
        .map_err(|err| PolicyStoreError::Policy(err.to_string()))
}

fn apply_global_edit(current: &str, edit: &PolicyEdit) -> Result<String, PolicyStoreError> {
    let mut doc = parse_doc(current)?;
    match edit {
        PolicyEdit::UpsertRule(rule) => upsert_rule(&mut doc, rule)?,
        PolicyEdit::RemoveRule { identity } => remove_rule(&mut doc, identity)?,
        PolicyEdit::UpsertRecognizer(rec) => upsert_recognizer(&mut doc, rec)?,
        PolicyEdit::RemoveRecognizer { name } => remove_recognizer(&mut doc, name)?,
    }
    Ok(doc.to_string())
}

fn apply_profile_edit(
    current: &str,
    edit: &PolicyEdit,
    store: &PolicyStore,
) -> Result<String, PolicyStoreError> {
    let mut doc = parse_doc(current)?;
    let global = store.document_for(&PolicyScope::Global)?;
    match edit {
        PolicyEdit::UpsertRule(rule) => {
            upsert_rule(&mut doc, rule)?;
        }
        PolicyEdit::RemoveRule { identity } => {
            if !global_contains_rule(&global, identity) {
                return Err(PolicyStoreError::Policy(format!(
                    "rule not found in global: {identity}"
                )));
            }
            add_remove_string(&mut doc, "remove_rules", identity);
            remove_override_rule(&mut doc, identity);
        }
        PolicyEdit::UpsertRecognizer(rec) => {
            upsert_recognizer(&mut doc, rec)?;
        }
        PolicyEdit::RemoveRecognizer { name } => {
            if !global_contains_recognizer(&global, name) {
                return Err(PolicyStoreError::Policy(format!(
                    "recognizer not found in global: {name}"
                )));
            }
            add_remove_string(&mut doc, "remove_recognizers", name);
            remove_override_recognizer(&mut doc, name);
        }
    }
    Ok(doc.to_string())
}

fn global_contains_rule(global: &str, identity: &str) -> bool {
    let Ok(doc) = global.parse::<DocumentMut>() else {
        return false;
    };
    merge::item_to_values(doc.get("rule")).iter().any(|value| {
        value
            .as_inline_table()
            .and_then(|t| rule_identity_from_table(t))
            .as_deref()
            == Some(identity)
    })
}

fn global_contains_recognizer(global: &str, name: &str) -> bool {
    let Ok(doc) = global.parse::<DocumentMut>() else {
        return false;
    };
    doc.get("policy")
        .and_then(|p| p.get("custom_recognizers"))
        .map(|item| {
            merge::item_to_values(Some(item))
                .iter()
                .any(|value| merge::value_str(value, "name") == Some(name))
        })
        .unwrap_or(false)
}

fn rule_identity_from_table(table: &dyn toml_edit::TableLike) -> Option<String> {
    let kind = table.get("kind").and_then(|i| i.as_str())?;
    Some(match kind {
        "class" => format!("class:{}", table.get("class").and_then(|i| i.as_str())?),
        "column" => format!("column:{}", table.get("column").and_then(|i| i.as_str())?),
        "default" => "default".to_string(),
        other => format!("kind:{other}"),
    })
}

fn write_rule_values(doc: &mut DocumentMut, values: Vec<Value>) {
    let mut array = Array::new();
    for value in values {
        array.push(value);
    }
    doc.insert("rule", Item::Value(Value::Array(array)));
}

fn upsert_rule(doc: &mut DocumentMut, rule: &RuleEdit) -> Result<(), PolicyStoreError> {
    let identity = rule_identity(&rule.kind, rule.class.as_deref(), rule.column.as_deref());
    let mut values = merge::item_to_values(doc.get("rule"));
    values.retain(|value| {
        value
            .as_inline_table()
            .and_then(|t| rule_identity_from_table(t))
            .as_deref()
            != Some(identity.as_str())
    });

    let mut table = InlineTable::new();
    table.insert("kind", rule.kind.clone().into());
    if let Some(class) = &rule.class {
        table.insert("class", class.clone().into());
    }
    if let Some(column) = &rule.column {
        table.insert("column", column.clone().into());
    }
    table.insert("action", rule.action.clone().into());
    values.push(Value::InlineTable(table));
    write_rule_values(doc, values);
    Ok(())
}

fn remove_rule(doc: &mut DocumentMut, identity: &str) -> Result<(), PolicyStoreError> {
    let mut values = merge::item_to_values(doc.get("rule"));
    values.retain(|value| {
        value
            .as_inline_table()
            .and_then(|t| rule_identity_from_table(t))
            .as_deref()
            != Some(identity)
    });
    write_rule_values(doc, values);
    Ok(())
}

fn policy_table_mut(
    doc: &mut DocumentMut,
) -> Result<&mut dyn toml_edit::TableLike, PolicyStoreError> {
    if !doc.contains_key("policy") {
        doc.insert("policy", Item::Table(Table::new()));
    }
    doc.get_mut("policy")
        .and_then(Item::as_table_like_mut)
        .ok_or_else(|| PolicyStoreError::Policy("policy table missing".into()))
}

fn read_recognizer_values(doc: &DocumentMut) -> Vec<Value> {
    doc.get("policy")
        .and_then(Item::as_table_like)
        .map(|policy| merge::item_to_values(policy.get("custom_recognizers")))
        .unwrap_or_default()
}

fn write_recognizer_values(
    doc: &mut DocumentMut,
    values: Vec<Value>,
) -> Result<(), PolicyStoreError> {
    let policy = policy_table_mut(doc)?;
    let mut array = Array::new();
    for value in values {
        array.push(value);
    }
    policy.insert("custom_recognizers", Item::Value(Value::Array(array)));
    Ok(())
}

fn upsert_recognizer(doc: &mut DocumentMut, rec: &RecognizerEdit) -> Result<(), PolicyStoreError> {
    let mut values = read_recognizer_values(doc);
    values.retain(|value| merge::value_str(value, "name") != Some(rec.name.as_str()));

    let mut table = InlineTable::new();
    table.insert("kind", rec.kind.clone().into());
    table.insert("name", rec.name.clone().into());
    if let Some(pattern) = &rec.pattern {
        table.insert("pattern", pattern.clone().into());
    }
    table.insert("class", rec.class.clone().into());
    if let Some(terms) = &rec.terms {
        let mut array = Array::new();
        for term in terms {
            array.push(term.as_str());
        }
        table.insert("terms", Value::Array(array));
    }
    if let Some(tf) = &rec.terms_file {
        table.insert("terms_file", tf.clone().into());
    }
    if let Some(tfc) = &rec.terms_from_context {
        table.insert("terms_from_context", tfc.clone().into());
    }
    if let Some(cs) = rec.case_sensitive {
        table.insert("case_sensitive", Value::from(cs));
    }
    if let Some(tf) = &rec.token_family {
        table.insert("token_family", tf.clone().into());
    }
    if let Some(st) = &rec.safety_tier {
        table.insert("safety_tier", st.clone().into());
    }
    values.push(Value::InlineTable(table));
    write_recognizer_values(doc, values)
}

fn remove_recognizer(doc: &mut DocumentMut, name: &str) -> Result<(), PolicyStoreError> {
    let mut values = read_recognizer_values(doc);
    values.retain(|value| merge::value_str(value, "name") != Some(name));
    write_recognizer_values(doc, values)
}

fn ensure_string_array<'a>(doc: &'a mut DocumentMut, key: &str) -> &'a mut Array {
    if !doc.contains_key(key) {
        doc.insert(key, Item::Value(Value::Array(Array::new())));
    }
    doc.get_mut(key)
        .and_then(Item::as_array_mut)
        .expect("string array")
}

fn add_remove_string(doc: &mut DocumentMut, key: &str, value: &str) {
    let array = ensure_string_array(doc, key);
    if !array.iter().any(|v| v.as_str() == Some(value)) {
        array.push(value);
    }
}

fn remove_override_rule(doc: &mut DocumentMut, identity: &str) {
    remove_nested_override(doc, "rules", |value| {
        value
            .as_inline_table()
            .and_then(|t| rule_identity_from_table(t))
            .as_deref()
            == Some(identity)
    });
}

fn remove_override_recognizer(doc: &mut DocumentMut, name: &str) {
    remove_nested_override(doc, "recognizers", |value| {
        merge::value_str(value, "name") == Some(name)
    });
}

fn remove_nested_override<F>(doc: &mut DocumentMut, key: &str, matches: F)
where
    F: Fn(&Value) -> bool,
{
    let Some(overrides) = doc.get_mut("overrides").and_then(Item::as_table_like_mut) else {
        return;
    };
    if overrides.get(key).is_none() {
        return;
    }
    let values = merge::item_to_values(overrides.get(key));
    let retained: Vec<Value> = values.into_iter().filter(|v| !matches(v)).collect();
    let mut array = Array::new();
    for value in retained {
        array.push(value);
    }
    overrides.insert(key, Item::Value(Value::Array(array)));
}
