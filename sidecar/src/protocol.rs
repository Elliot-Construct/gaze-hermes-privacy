//! Protocol constants shared by the sidecar and its clients.

use serde::Serialize;

pub const PROTOCOL_VERSION: u32 = 1;
pub const SIDECAR_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Serialize)]
pub struct StatusResponse {
    pub status: &'static str,
    pub protocol_version: u32,
    pub sidecar_version: &'static str,
}
