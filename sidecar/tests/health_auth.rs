use axum::body::Body;
use axum::http::{Request, StatusCode};
use gaze_hermes_sidecar::api::build_router;
use gaze_hermes_sidecar::auth::AuthState;
use serde_json::Value;
use tower::util::ServiceExt;

async fn test_app(token: &str) -> axum::Router {
    build_router(AuthState::new(token))
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
