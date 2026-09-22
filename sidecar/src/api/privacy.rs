use std::sync::atomic::Ordering;
use std::time::Instant;

use axum::extract::State;
use axum::Json;

use super::{ApiError, AppState};
use crate::protocol::{
    CleanRequest, CleanResponse, DetectionCount, RestoreRequest, RestoreResponse, TextField,
};

pub async fn clean(
    State(state): State<AppState>,
    Json(request): Json<CleanRequest>,
) -> Result<Json<CleanResponse>, ApiError> {
    let started = Instant::now();
    state
        .metrics
        .clean_requests
        .fetch_add(1, Ordering::Relaxed);
    match run_clean(&state, request).await {
        Ok(response) => {
            observe_latency(
                &state.metrics.clean_latency_ms_sum,
                &state.metrics.clean_latency_ms_max,
                started,
            );
            Ok(Json(response))
        }
        Err(err) => {
            state.metrics.clean_errors.fetch_add(1, Ordering::Relaxed);
            Err(err)
        }
    }
}

pub async fn restore(
    State(state): State<AppState>,
    Json(request): Json<RestoreRequest>,
) -> Result<Json<RestoreResponse>, ApiError> {
    let started = Instant::now();
    state
        .metrics
        .restore_requests
        .fetch_add(1, Ordering::Relaxed);
    match run_restore(&state, request).await {
        Ok(response) => {
            observe_latency(
                &state.metrics.restore_latency_ms_sum,
                &state.metrics.restore_latency_ms_max,
                started,
            );
            Ok(Json(response))
        }
        Err(err) => {
            state.metrics.restore_errors.fetch_add(1, Ordering::Relaxed);
            Err(err)
        }
    }
}

async fn run_clean(state: &AppState, request: CleanRequest) -> Result<CleanResponse, ApiError> {
    let session_key = request.namespace.session_key();
    let effective = super::effective_for_profile(&state.policies, &request.namespace.profile_id)?;
    let handle = state.sessions.get_or_restore(&session_key).await?;
    let session = handle.session.lock().await;
    let locale_tags = super::locale_tags_for(&effective.policy);
    let dictionaries = gaze::DictionaryBundle::default();
    let mut tx = session.begin_transaction();
    let mut cleaned = Vec::with_capacity(request.fields.len());
    let mut detections: Vec<DetectionCount> = Vec::new();

    for field in &request.fields {
        let text = effective
            .pipeline
            .protect_text_transaction(
                &mut tx,
                &field.text,
                gaze::ProtectionContext::strict(&locale_tags, &dictionaries),
            )
            .map_err(|_| ApiError::Privacy)?;
        collect_detections(&field.text, &text, &mut detections);
        cleaned.push(TextField {
            path: field.path.clone(),
            text,
        });
    }

    tx.commit().map_err(|_| ApiError::Privacy)?;
    drop(session);
    state.sessions.persist(&session_key).await?;

    Ok(CleanResponse {
        fields: cleaned,
        detections: merge_detections(detections),
        policy_version: effective.hash,
    })
}

async fn run_restore(state: &AppState, request: RestoreRequest) -> Result<RestoreResponse, ApiError> {
    let session_key = request.namespace.session_key();
    let handle = state.sessions.get_or_restore(&session_key).await?;
    let session = handle.session.lock().await;
    let mut restored = Vec::with_capacity(request.fields.len());
    for field in &request.fields {
        let text = session
            .restore_strict_text(&field.text)
            .map_err(|_| ApiError::StrictRestore)?;
        restored.push(TextField {
            path: field.path.clone(),
            text,
        });
    }
    drop(session);
    Ok(RestoreResponse { fields: restored })
}

fn observe_latency(
    sum: &std::sync::atomic::AtomicU64,
    max: &std::sync::atomic::AtomicU64,
    started: Instant,
) {
    let ms = started.elapsed().as_millis() as u64;
    sum.fetch_add(ms, Ordering::Relaxed);
    max.fetch_max(ms, Ordering::Relaxed);
}

fn collect_detections(original: &str, protected: &str, out: &mut Vec<DetectionCount>) {
    if original == protected {
        return;
    }
    if let Some(class) = guess_class(original, protected) {
        out.push(DetectionCount { class, count: 1 });
    }
}

fn guess_class(original: &str, protected: &str) -> Option<String> {
    let protected_words: std::collections::HashSet<&str> = protected.split_whitespace().collect();
    for word in original.split_whitespace() {
        if protected_words.contains(word) {
            continue;
        }
        if word.contains('@') {
            return Some("email".into());
        }
        let trimmed = word.trim_matches(|c: char| !c.is_alphanumeric());
        if trimmed
            .chars()
            .next()
            .map(|c| c.is_ascii_uppercase())
            .unwrap_or(false)
            && trimmed.len() > 1
        {
            return Some("name".into());
        }
    }
    None
}

fn merge_detections(detections: Vec<DetectionCount>) -> Vec<DetectionCount> {
    let mut merged: Vec<DetectionCount> = Vec::new();
    for detection in detections {
        if let Some(existing) = merged.iter_mut().find(|d| d.class == detection.class) {
            existing.count += detection.count;
        } else {
            merged.push(detection);
        }
    }
    merged
}


