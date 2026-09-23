use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use gaze_hermes_sidecar::api::{build_router, AppState, Metrics};
use gaze_hermes_sidecar::auth::AuthState;
use gaze_hermes_sidecar::policies::PolicyStore;
use gaze_hermes_sidecar::sessions::SessionRegistry;
use gaze_hermes_sidecar::streaming::StreamManager;
use serde_json::Value;
use tower::util::ServiceExt;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gaze-health-{}-{}-{}",
        tag,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join("policies")).unwrap();
    std::fs::create_dir_all(dir.join("sessions")).unwrap();
    std::fs::write(
        dir.join("policies").join("global.toml"),
        gaze_hermes_sidecar::config::DEFAULT_GLOBAL_POLICY,
    )
    .unwrap();
    dir
}

async fn test_app(token: &str) -> axum::Router {
    test_app_in(token, &temp_dir("health")).await
}

async fn test_app_in(token: &str, dir: &std::path::Path) -> axum::Router {
    let policies = Arc::new(PolicyStore::open(&dir.join("policies")).unwrap());
    let sessions = Arc::new(SessionRegistry::open(&dir.join("sessions")).unwrap());
    let state = AppState::new(
        AuthState::new(token),
        policies,
        sessions,
        Arc::new(Metrics::default()),
        Arc::new(StreamManager::default()),
    );
    build_router(state)
}

async fn body_json(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json")
}

#[tokio::test]
async fn status_requires_bearer_token() {
    let app = test_app("secret-token").await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn health_is_minimal_and_protocol_is_explicit() {
    let app = test_app("secret-token").await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["status"], "ok");
    assert_eq!(body["protocol_version"], 1);
    assert!(body.get("sessions").is_none());
}

#[tokio::test]
async fn status_with_valid_bearer_returns_protocol() {
    let app = test_app("secret-token").await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/status")
                .header("Authorization", "Bearer secret-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["status"], "ok");
    assert_eq!(body["protocol_version"], 1);
    assert!(body["sidecar_version"].is_string());
}

#[tokio::test]
async fn wrong_length_bearer_is_rejected() {
    let app = test_app("secret-token").await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/status")
                .header("Authorization", "Bearer short")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
