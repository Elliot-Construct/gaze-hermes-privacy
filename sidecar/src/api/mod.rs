//! HTTP API surface.

pub mod metrics;
pub mod policies;
pub mod privacy;
pub mod sessions;
pub mod status;

use axum::routing::{delete, get, post};
use axum::Router;

use crate::auth::{require_bearer, AuthState};
use crate::policies::{PolicyStore, PolicyStoreError};
use crate::sessions::SessionRegistry;

pub use metrics::Metrics;

#[derive(Clone)]
pub struct AppState {
    pub auth: AuthState,
    pub policies: std::sync::Arc<PolicyStore>,
    pub sessions: std::sync::Arc<SessionRegistry>,
    pub metrics: std::sync::Arc<Metrics>,
}

impl AppState {
    pub fn new(
        auth: AuthState,
        policies: std::sync::Arc<PolicyStore>,
        sessions: std::sync::Arc<SessionRegistry>,
        metrics: std::sync::Arc<Metrics>,
    ) -> Self {
        Self {
            auth,
            policies,
            sessions,
            metrics,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("not found")]
    NotFound,
    #[error("conflict")]
    Conflict,
    #[error("strict restore failed")]
    StrictRestore,
    #[error("recovery blocked")]
    RecoveryBlocked,
    #[error("validation failed: {0}")]
    Validation(String),
    #[error("privacy error")]
    Privacy,
    #[error("internal error: {0}")]
    Internal(String),
}

impl ApiError {
    pub fn code(&self) -> &'static str {
        match self {
            ApiError::NotFound => "not_found",
            ApiError::Conflict => "conflict",
            ApiError::StrictRestore => "strict_restore_failed",
            ApiError::RecoveryBlocked => "recovery_blocked",
            ApiError::Validation(_) => "validation_failed",
            ApiError::Privacy => "privacy_error",
            ApiError::Internal(_) => "internal_error",
        }
    }
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        use axum::http::StatusCode;
        use axum::Json;
        let status = match self {
            ApiError::NotFound => StatusCode::NOT_FOUND,
            ApiError::Conflict => StatusCode::CONFLICT,
            ApiError::StrictRestore | ApiError::RecoveryBlocked | ApiError::Validation(_) => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            ApiError::Privacy | ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let mut body = serde_json::Map::new();
        body.insert(
            "code".into(),
            serde_json::Value::String(self.code().to_string()),
        );
        if let ApiError::Validation(message) = &self {
            body.insert(
                "message".into(),
                serde_json::Value::String(message.clone()),
            );
        }
        (status, Json(serde_json::Value::Object(body))).into_response()
    }
}

impl From<PolicyStoreError> for ApiError {
    fn from(err: PolicyStoreError) -> Self {
        match err {
            PolicyStoreError::Conflict => ApiError::Conflict,
            PolicyStoreError::UnknownProfile(_) => ApiError::NotFound,
            PolicyStoreError::InvalidProfileId(_)
            | PolicyStoreError::Policy(_)
            | PolicyStoreError::Build(_)
            | PolicyStoreError::Merge(_) => ApiError::Validation(err.to_string()),
            PolicyStoreError::Io(_) | PolicyStoreError::UnknownBundledRulepack(_) => {
                ApiError::Internal(err.to_string())
            }
        }
    }
}

impl From<crate::sessions::StoreError> for ApiError {
    fn from(err: crate::sessions::StoreError) -> Self {
        use crate::sessions::StoreError;
        match err {
            StoreError::RecoveryBlocked => ApiError::RecoveryBlocked,
            StoreError::Session(_)
            | StoreError::Io(_)
            | StoreError::Encrypt
            | StoreError::Decrypt
            | StoreError::Integrity => ApiError::Internal(err.to_string()),
        }
    }
}

pub fn effective_for_profile(
    policies: &PolicyStore,
    profile_id: &str,
) -> Result<crate::policies::EffectivePolicy, PolicyStoreError> {
    match policies.effective(Some(profile_id)) {
        Ok(eff) => Ok(eff),
        Err(PolicyStoreError::UnknownProfile(_)) => policies.effective(None),
        Err(err) => Err(err),
    }
}

pub fn locale_tags_for(policy: &gaze::Policy) -> Vec<gaze::LocaleTag> {
    policy.locale.clone().unwrap_or_default()
}

pub fn build_router(state: AppState) -> Router {
    let auth = state.auth.clone();
    let public = Router::new().route("/healthz", get(status::healthz));
    let protected = Router::new()
        .route("/v1/status", get(status::status))
        .route("/v1/clean", post(privacy::clean))
        .route("/v1/restore", post(privacy::restore))
        .route("/v1/policies/validate", post(policies::validate_policy))
        .route("/v1/policies/test", post(policies::test_policy))
        .route("/v1/policies/edit", post(policies::edit_policy))
        .route("/v1/policies/apply", post(policies::apply_policy))
        .route("/v1/policies/effective", get(policies::effective_policy))
        .route("/v1/sessions", get(sessions::list_sessions))
        .route("/v1/sessions/{profile}/{session}", get(sessions::get_session))
        .route(
            "/v1/sessions/{profile}/{session}/recover",
            post(sessions::recover_session),
        )
        .route(
            "/v1/sessions/{profile}/{session}",
            delete(sessions::delete_session),
        )
        .route("/v1/metrics", get(metrics::metrics_handler))
        .layer(axum::middleware::from_fn_with_state(
            state.auth.clone(),
            require_bearer,
        ))
        .with_state(state);
    public.merge(protected).with_state(auth)
}
