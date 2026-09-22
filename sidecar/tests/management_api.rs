use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use gaze_hermes_sidecar::api::{build_router, AppState, Metrics};
use gaze_hermes_sidecar::auth::AuthState;
use gaze_hermes_sidecar::policies::{PolicyScope, PolicyStore};
use gaze_hermes_sidecar::sessions::{snapshot_path, SessionKey, SessionRegistry};
use gaze_hermes_sidecar::streaming::StreamManager;
use serde_json::Value;
use tower::util::ServiceExt;

const TOKEN: &str = "secret-token";
const EMAIL: &str = "alice@example.invalid";

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gaze-mgmt-api-{tag}-{}-{}",
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
        Arc::new(StreamManager::default()),
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

async fn send(
    app: axum::Router,
    method: &str,
    uri: &str,
    auth: bool,
    body: Option<&Value>,
) -> axum::response::Response {
    let mut builder = Request::builder().method(method).uri(uri);
    if auth {
        builder = builder.header("Authorization", format!("Bearer {TOKEN}"));
    }
    let request = match body {
        Some(value) => builder
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(value).unwrap()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    app.oneshot(request).await.unwrap()
}

fn namespace(session: &str) -> Value {
    serde_json::json!({
        "profile_id": "default",
        "session_id": session,
        "request_id": "req-1",
    })
}

#[tokio::test]
async fn every_management_route_requires_bearer() {
    let dir = temp_dir("auth");
    let state = test_state(&dir);

    let cases: Vec<(&str, &str, Option<Value>)> = vec![
        ("GET", "/v1/status", None),
        ("POST", "/v1/clean", Some(serde_json::json!({"namespace": namespace("s"), "fields": []}))),
        ("POST", "/v1/restore", Some(serde_json::json!({"namespace": namespace("s"), "fields": []}))),
        ("POST", "/v1/policies/validate", Some(serde_json::json!({"toml": ""}))),
        ("POST", "/v1/policies/test", Some(serde_json::json!({"namespace": namespace("s"), "fields": []}))),
        ("POST", "/v1/policies/edit", Some(serde_json::json!({"scope": "global", "expected_hash": "", "edit": {"UpsertRule": {"kind": "class", "class": "email", "action": "tokenize"}}}))),
        ("POST", "/v1/policies/apply", Some(serde_json::json!({"scope": "global", "expected_hash": "", "toml": ""}))),
        ("GET", "/v1/policies/effective", None),
        ("GET", "/v1/sessions", None),
        ("GET", "/v1/sessions/p/s", None),
        ("POST", "/v1/sessions/p/s/recover", None),
        ("DELETE", "/v1/sessions/p/s", None),
        ("GET", "/v1/metrics", None),
    ];

    for (method, uri, body) in cases {
        let response = send(app(&state), method, uri, false, body.as_ref()).await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {uri} without bearer"
        );
    }

    let response = send(app(&state), "GET", "/healthz", false, None).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn invalid_policy_apply_leaves_active_hash_unchanged() {
    let dir = temp_dir("invalid-apply");
    let state = test_state(&dir);

    let before = body_json(
        send(app(&state), "GET", "/v1/policies/effective", true, None).await,
    )
    .await;
    let hash = before["hash"].as_str().unwrap().to_string();

    let invalid = serde_json::json!({
        "scope": "global",
        "expected_hash": hash,
        "toml": "schema_version = \"0.2.0\"\n",
    });
    let response = send(app(&state), "POST", "/v1/policies/apply", true, Some(&invalid)).await;
    assert_ne!(response.status(), StatusCode::OK);

    let after = body_json(
        send(app(&state), "GET", "/v1/policies/effective", true, None).await,
    )
    .await;
    assert_eq!(before["hash"], after["hash"]);
}

#[tokio::test]
async fn stale_expected_hash_returns_conflict() {
    let dir = temp_dir("stale-hash");
    let state = test_state(&dir);

    let request = serde_json::json!({
        "scope": "global",
        "expected_hash": "deadbeef",
        "toml": gaze_hermes_sidecar::config::DEFAULT_GLOBAL_POLICY,
    });
    let response = send(app(&state), "POST", "/v1/policies/apply", true, Some(&request)).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn session_list_and_get_never_leak_raw_values() {
    let dir = temp_dir("session-leak");
    let state = test_state(&dir);

    let clean = serde_json::json!({
        "namespace": namespace("leak-1"),
        "fields": [{"path": "/a", "text": format!("Email {EMAIL}")}],
    });
    let response = send(app(&state), "POST", "/v1/clean", true, Some(&clean)).await;
    assert_eq!(response.status(), StatusCode::OK);

    let listed = body_json(send(app(&state), "GET", "/v1/sessions", true, None).await).await;
    let raw = serde_json::to_string(&listed).unwrap();
    assert!(!raw.contains(EMAIL));
    assert!(!raw.contains("alice"));
    assert!(!raw.contains("mappings"));
    assert!(!raw.contains("tokens"));

    let single = body_json(
        send(app(&state), "GET", "/v1/sessions/default/leak-1", true, None).await,
    )
    .await;
    let raw = serde_json::to_string(&single).unwrap();
    assert!(!raw.contains(EMAIL));
    assert!(!raw.contains("mappings"));
    assert_eq!(single["profile_id"], "default");
    assert_eq!(single["session_id"], "leak-1");
    assert_eq!(single["has_snapshot"], true);
}

#[tokio::test]
async fn delete_session_removes_snapshot() {
    let dir = temp_dir("delete-snap");
    let state = test_state(&dir);

    let clean = serde_json::json!({
        "namespace": namespace("del-1"),
        "fields": [{"path": "/a", "text": format!("Email {EMAIL}")}],
    });
    let response = send(app(&state), "POST", "/v1/clean", true, Some(&clean)).await;
    assert_eq!(response.status(), StatusCode::OK);

    let key = SessionKey::new("default", "del-1");
    let path = snapshot_path(&dir.join("sessions"), &key);
    assert!(path.exists());

    let response = send(app(&state), "DELETE", "/v1/sessions/default/del-1", true, None).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(!path.exists());

    let response = send(app(&state), "GET", "/v1/sessions/default/del-1", true, None).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn metrics_returns_counters_json() {
    let dir = temp_dir("metrics");
    let state = test_state(&dir);

    let clean = serde_json::json!({
        "namespace": namespace("m-1"),
        "fields": [{"path": "/a", "text": format!("Email {EMAIL}")}],
    });
    let response = send(app(&state), "POST", "/v1/clean", true, Some(&clean)).await;
    assert_eq!(response.status(), StatusCode::OK);

    let metrics = body_json(send(app(&state), "GET", "/v1/metrics", true, None).await).await;
    assert_eq!(metrics["clean_requests"], 1);
    assert_eq!(metrics["clean_errors"], 0);
    assert!(metrics.get("restore_requests").is_some());
    assert!(metrics.get("policy_apply_conflicts").is_some());
}

#[tokio::test]
async fn recover_reports_success_for_existing_session() {
    let dir = temp_dir("recover");
    let state = test_state(&dir);

    let clean = serde_json::json!({
        "namespace": namespace("r-1"),
        "fields": [{"path": "/a", "text": format!("Email {EMAIL}")}],
    });
    let response = send(app(&state), "POST", "/v1/clean", true, Some(&clean)).await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = send(app(&state), "POST", "/v1/sessions/default/r-1/recover", true, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["recovered"], true);
}

#[tokio::test]
async fn policy_edit_returns_candidate_without_changing_active() {
    let dir = temp_dir("edit-candidate");
    let state = test_state(&dir);

    let before = body_json(
        send(app(&state), "GET", "/v1/policies/effective", true, None).await,
    )
    .await;
    let hash = before["hash"].as_str().unwrap().to_string();

    let edit = serde_json::json!({
        "scope": "global",
        "expected_hash": hash,
        "edit": {"UpsertRule": {"kind": "class", "class": "phone", "action": "tokenize"}},
    });
    let response = send(app(&state), "POST", "/v1/policies/edit", true, Some(&edit)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let candidate = body_json(response).await;
    assert!(candidate["toml"].as_str().unwrap().contains("phone"));

    let after = body_json(
        send(app(&state), "GET", "/v1/policies/effective", true, None).await,
    )
    .await;
    assert_eq!(before["hash"], after["hash"]);
    assert!(!after["toml"].as_str().unwrap().contains("phone"));
}

#[tokio::test]
async fn policy_scope_deserializes_from_wire_format() {
    let scope: PolicyScope = serde_json::from_value(serde_json::json!("global")).unwrap();
    assert_eq!(scope, PolicyScope::Global);
    let scope: PolicyScope =
        serde_json::from_value(serde_json::json!({"profile": "team-a"})).unwrap();
    assert_eq!(scope, PolicyScope::Profile("team-a".into()));
}
