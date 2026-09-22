//! Health and status handlers.

use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};

use crate::auth::AuthState;
use crate::protocol::{StatusResponse, PROTOCOL_VERSION};

pub async fn healthz() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "protocol_version": PROTOCOL_VERSION,
    }))
}

pub async fn status(_state: State<AuthState>) -> Json<StatusResponse> {
    Json(StatusResponse {
        status: "ok",
        protocol_version: PROTOCOL_VERSION,
        sidecar_version: crate::protocol::SIDECAR_VERSION,
    })
}
