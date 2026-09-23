//! Canonical field protocol shared by sidecar and clients.

use serde::{Deserialize, Serialize};

pub use crate::sessions::SessionKey;

pub const PROTOCOL_VERSION: u32 = 1;
pub const SIDECAR_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Serialize)]
pub struct StatusResponse {
    pub status: &'static str,
    pub protocol_version: u32,
    pub sidecar_version: &'static str,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RequestNamespace {
    pub profile_id: String,
    pub session_id: String,
    pub request_id: String,
}

impl RequestNamespace {
    pub fn session_key(&self) -> SessionKey {
        SessionKey::new(&self.profile_id, &self.session_id)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TextField {
    pub path: String,
    pub text: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CleanRequest {
    pub namespace: RequestNamespace,
    pub fields: Vec<TextField>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RestoreRequest {
    pub namespace: RequestNamespace,
    pub fields: Vec<TextField>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DetectionCount {
    pub class: String,
    pub count: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CleanResponse {
    pub fields: Vec<TextField>,
    pub detections: Vec<DetectionCount>,
    pub policy_version: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RestoreResponse {
    pub fields: Vec<TextField>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PolicyDocumentRequest {
    pub toml: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PolicyApplyRequest {
    pub scope: crate::policies::PolicyScope,
    pub expected_hash: String,
    pub toml: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PolicyEditRequest {
    pub scope: crate::policies::PolicyScope,
    pub expected_hash: String,
    pub edit: crate::policies::PolicyEdit,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PolicyTestRequest {
    pub namespace: RequestNamespace,
    pub fields: Vec<TextField>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EffectivePolicyResponse {
    pub toml: String,
    pub hash: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PolicyTestFieldResult {
    pub path: String,
    pub original_present: bool,
    pub protected: String,
    pub restored: String,
    pub round_trip_ok: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PolicyTestResponse {
    pub fields: Vec<PolicyTestFieldResult>,
}
