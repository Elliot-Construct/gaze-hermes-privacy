use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;

use super::{ApiError, AppState};
use crate::policies::ValidationResult;
use crate::protocol::{
    EffectivePolicyResponse, PolicyApplyRequest, PolicyDocumentRequest, PolicyEditRequest,
    PolicyTestFieldResult, PolicyTestRequest, PolicyTestResponse,
};

pub async fn validate_policy(
    State(state): State<AppState>,
    Json(request): Json<PolicyDocumentRequest>,
) -> Json<ValidationResult> {
    Json(state.policies.validate(&request.toml))
}

pub async fn edit_policy(
    State(state): State<AppState>,
    Json(request): Json<PolicyEditRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let edited = state
        .policies
        .edit(request.scope, &request.expected_hash, request.edit)?;
    Ok(Json(
        serde_json::json!({ "toml": edited.toml, "hash": edited.hash }),
    ))
}

pub async fn apply_policy(
    State(state): State<AppState>,
    Json(request): Json<PolicyApplyRequest>,
) -> Result<Json<EffectivePolicyResponse>, ApiError> {
    let result = state
        .policies
        .apply(request.scope, &request.expected_hash, &request.toml);
    match result {
        Ok(effective) => Ok(Json(EffectivePolicyResponse {
            toml: effective.toml,
            hash: effective.hash,
        })),
        Err(crate::policies::PolicyStoreError::Conflict) => {
            state.metrics.record_policy_conflict();
            Err(ApiError::Conflict)
        }
        Err(err) => Err(err.into()),
    }
}

#[derive(Deserialize)]
pub struct EffectiveQuery {
    pub profile_id: Option<String>,
}

pub async fn effective_policy(
    State(state): State<AppState>,
    Query(query): Query<EffectiveQuery>,
) -> Result<Json<EffectivePolicyResponse>, ApiError> {
    let effective = match query.profile_id.as_deref() {
        Some(profile_id) => {
            if let Err(err) = crate::policies::validate_profile_id_public(profile_id) {
                return Err(err.into());
            }
            super::effective_for_profile(&state.policies, profile_id)?
        }
        None => state.policies.effective(None)?,
    };
    Ok(Json(EffectivePolicyResponse {
        toml: effective.toml,
        hash: effective.hash,
    }))
}

pub async fn test_policy(
    State(state): State<AppState>,
    Json(request): Json<PolicyTestRequest>,
) -> Result<Json<PolicyTestResponse>, ApiError> {
    let effective = super::effective_for_profile(&state.policies, &request.namespace.profile_id)?;
    let session = gaze::Session::new(gaze::Scope::Conversation(format!(
        "policy-test-{}-{}",
        request.namespace.session_id, request.namespace.request_id
    )))
    .map_err(|err| ApiError::Internal(err.to_string()))?;

    let locale_tags = super::locale_tags_for(&effective.policy);
    let dictionaries = gaze::DictionaryBundle::default();
    let mut tx = session.begin_transaction();
    let mut results = Vec::with_capacity(request.fields.len());
    for field in &request.fields {
        let protected = effective
            .pipeline
            .protect_text_transaction(
                &mut tx,
                &field.text,
                gaze::ProtectionContext::strict(&locale_tags, &dictionaries),
            )
            .map_err(|_| ApiError::Privacy)?;
        results.push((field.path.clone(), field.text.clone(), protected));
    }
    tx.commit().map_err(|_| ApiError::Privacy)?;

    let mut fields = Vec::with_capacity(results.len());
    for (path, original, protected) in results {
        let restored = session
            .restore_strict_text(&protected)
            .map_err(|_| ApiError::StrictRestore)?;
        let original_present = protected != original;
        fields.push(PolicyTestFieldResult {
            path,
            original_present,
            protected,
            round_trip_ok: restored == original,
            restored,
        });
    }

    Ok(Json(PolicyTestResponse { fields }))
}
