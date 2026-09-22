use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use gaze_hermes_sidecar::api::{build_router, AppState, Metrics};
use gaze_hermes_sidecar::auth::AuthState;
use gaze_hermes_sidecar::policies::PolicyStore;
use gaze_hermes_sidecar::sessions::SessionRegistry;
use serde_json::Value;
use tower::util::ServiceExt;

const TOKEN: &str = "secret-token";
const EMAIL: &str = "alice@example.invalid";

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gaze-privacy-api-{tag}-{}-{}",
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

fn test_state(dir: &std::path::Path) -> AppState {
    let policies = Arc::new(PolicyStore::open(&dir.join("policies")).unwrap());
    let sessions = Arc::new(SessionRegistry::open(&dir.join("sessions")).unwrap());
    AppState::new(
        AuthState::new(TOKEN),
        policies,
        sessions,
        Arc::new(Metrics::default()),
    )
}

fn app(state: &AppState) -> axum::Router {
    build_router(state.clone())
}

async fn body_json(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json")
}

async fn body_text(response: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body");
    String::from_utf8_lossy(&bytes).into_owned()
}

fn namespace(session: &str) -> serde_json::Value {
    serde_json::json!({
        "profile_id": "default",
        "session_id": session,
        "request_id": "req-1",
    })
}

async fn post_json(app: axum::Router, uri: &str, body: &Value) -> axum::response::Response {
    app.oneshot(
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .header("Authorization", format!("Bearer {TOKEN}"))
            .body(Body::from(serde_json::to_vec(body).unwrap()))
            .unwrap(),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn clean_round_trip_email_tokenizes_and_restores() {
    let dir = temp_dir("round-trip");
    let state = test_state(&dir);

    let clean_body = serde_json::json!({
        "namespace": namespace("s1"),
        "fields": [{"path": "/messages/0/content", "text": format!("Contact {EMAIL}")}],
    });
    let response = post_json(app(&state), "/v1/clean", &clean_body).await;
    assert_eq!(response.status(), StatusCode::OK);
    let cleaned = body_json(response).await;
    let protected = cleaned["fields"][0]["text"].as_str().unwrap();
    assert_ne!(protected, format!("Contact {EMAIL}"));
    assert!(!protected.contains(EMAIL));
    assert!(cleaned["policy_version"].is_string());
    let detections = cleaned["detections"].as_array().expect("detections");
    assert!(!detections.is_empty());
    for detection in detections {
        assert!(detection.get("class").is_some());
        assert!(detection.get("count").is_some());
        assert!(detection.as_object().unwrap().len() == 2);
    }
    let raw = serde_json::to_string(&cleaned).unwrap();
    assert!(!raw.contains("mappings"));
    assert!(!raw.contains(EMAIL));

    let restore_body = serde_json::json!({
        "namespace": namespace("s1"),
        "fields": [{"path": "/messages/0/content", "text": protected}],
    });
    let response = post_json(app(&state), "/v1/restore", &restore_body).await;
    assert_eq!(response.status(), StatusCode::OK);
    let restored = body_json(response).await;
    assert_eq!(
        restored["fields"][0]["text"].as_str().unwrap(),
        format!("Contact {EMAIL}")
    );
}

#[tokio::test]
async fn multi_field_clean_commits_all_or_nothing() {
    let dir = temp_dir("multi-clean");
    let state = test_state(&dir);

    let clean_body = serde_json::json!({
        "namespace": namespace("s2"),
        "fields": [
            {"path": "/a", "text": format!("Email {EMAIL}")},
            {"path": "/b", "text": "plain text"},
        ],
    });
    let response = post_json(app(&state), "/v1/clean", &clean_body).await;
    assert_eq!(response.status(), StatusCode::OK);
    let cleaned = body_json(response).await;
    let fields = cleaned["fields"].as_array().unwrap();
    assert_eq!(fields.len(), 2);
    assert!(!fields[0]["text"].as_str().unwrap().contains(EMAIL));

    let session_dir = dir.join("sessions");
    let enc_count = std::fs::read_dir(&session_dir)
        .unwrap()
        .filter(|e| {
            e.as_ref().unwrap().path().extension().and_then(|x| x.to_str()) == Some("enc")
        })
        .count();
    assert_eq!(enc_count, 1);
}

#[tokio::test]
async fn restore_unknown_token_returns_422_without_raw_text() {
    let dir = temp_dir("strict-restore");
    let state = test_state(&dir);

    let clean_body = serde_json::json!({
        "namespace": namespace("s3"),
        "fields": [{"path": "/a", "text": format!("Email {EMAIL}")}],
    });
    let response = post_json(app(&state), "/v1/clean", &clean_body).await;
    assert_eq!(response.status(), StatusCode::OK);
    let cleaned = body_json(response).await;
    let protected = cleaned["fields"][0]["text"].as_str().unwrap().to_string();

    let restore_body = serde_json::json!({
        "namespace": namespace("s3"),
        "fields": [
            {"path": "/ok", "text": protected},
            {"path": "/bad", "text": "<NotOwnedToken_999999>"},
        ],
    });
    let response = post_json(app(&state), "/v1/restore", &restore_body).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let text = body_text(response).await;
    assert!(text.contains("strict_restore_failed"));
    assert!(!text.contains(EMAIL));
    assert!(!text.contains("alice"));

    let restore_ok = serde_json::json!({
        "namespace": namespace("s3"),
        "fields": [{"path": "/ok", "text": protected}],
    });
    let response = post_json(app(&state), "/v1/restore", &restore_ok).await;
    assert_eq!(response.status(), StatusCode::OK);
    let restored = body_json(response).await;
    assert_eq!(
        restored["fields"][0]["text"].as_str().unwrap(),
        format!("Email {EMAIL}")
    );
}

#[tokio::test]
async fn policy_test_round_trips_without_changing_effective_hash() {
    let dir = temp_dir("policy-test");
    let state = test_state(&dir);

    let effective_before = app(&state)
        .oneshot(
            Request::builder()
                .uri("/v1/policies/effective")
                .header("Authorization", format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(effective_before.status(), StatusCode::OK);
    let before = body_json(effective_before).await;

    let test_body = serde_json::json!({
        "namespace": namespace("s4"),
        "fields": [{"path": "/messages/0/content", "text": format!("Contact {EMAIL}")}],
    });
    let response = post_json(app(&state), "/v1/policies/test", &test_body).await;
    assert_eq!(response.status(), StatusCode::OK);
    let tested = body_json(response).await;
    let field = &tested["fields"][0];
    assert_eq!(field["path"], "/messages/0/content");
    assert_eq!(field["original_present"], true);
    assert_eq!(field["round_trip_ok"], true);
    assert_eq!(
        field["restored"].as_str().unwrap(),
        format!("Contact {EMAIL}")
    );
    assert!(!field["protected"].as_str().unwrap().contains(EMAIL));

    let effective_after = app(&state)
        .oneshot(
            Request::builder()
                .uri("/v1/policies/effective")
                .header("Authorization", format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let after = body_json(effective_after).await;
    assert_eq!(before["hash"], after["hash"]);

    let listed = app(&state)
        .oneshot(
            Request::builder()
                .uri("/v1/sessions")
                .header("Authorization", format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let sessions = body_json(listed).await;
    let raw = serde_json::to_string(&sessions).unwrap();
    assert!(!raw.contains(EMAIL));
}
