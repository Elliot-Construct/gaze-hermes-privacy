use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use super::{ApiError, AppState};
use crate::sessions::SessionKey;

pub async fn list_sessions(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let sessions = state.sessions.list().await?;
    Ok(Json(serde_json::json!({ "sessions": sessions })))
}

pub async fn get_session(
    State(state): State<AppState>,
    Path((profile_id, session_id)): Path<(String, String)>,
) -> Result<Json<crate::sessions::SessionMetadata>, ApiError> {
    let key = SessionKey::new(profile_id, session_id);
    let metadata = state.sessions.metadata(&key).await.map_err(|err| match err {
        crate::sessions::StoreError::Session(_) => ApiError::NotFound,
        other => other.into(),
    })?;
    Ok(Json(metadata))
}

pub async fn recover_session(
    State(state): State<AppState>,
    Path((profile_id, session_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let key = SessionKey::new(profile_id, session_id);
    state.sessions.get_or_restore(&key).await?;
    Ok(Json(serde_json::json!({ "recovered": true })))
}

pub async fn delete_session(
    State(state): State<AppState>,
    Path((profile_id, session_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let key = SessionKey::new(profile_id, session_id);
    let path = crate::sessions::snapshot_path(
        &state.sessions.dir(),
        &key,
    );
    if !path.exists() && state.sessions.metadata(&key).await.is_err() {
        return Err(ApiError::NotFound);
    }
    state.sessions.delete(&key).await?;
    Ok(StatusCode::NO_CONTENT)
}
