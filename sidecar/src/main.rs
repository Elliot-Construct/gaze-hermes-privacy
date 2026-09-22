//! Sidecar entrypoint: bind loopback, optional ready-file rendezvous, serve HTTP.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use clap::Parser;
use tracing::info;

use gaze_hermes_sidecar::api::{build_router, AppState, Metrics};
use gaze_hermes_sidecar::auth::AuthState;
use gaze_hermes_sidecar::config::Config;
use gaze_hermes_sidecar::policies::PolicyStore;
use gaze_hermes_sidecar::protocol::PROTOCOL_VERSION;
use gaze_hermes_sidecar::sessions::SessionRegistry;
use gaze_hermes_sidecar::streaming::StreamManager;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = Config::parse();
    config.validate_bind()?;
    let token = config.load_token()?;
    let auth = AuthState::new(&token);

    let data_dir = config.bootstrap_data_dir()?;
    let policies = Arc::new(PolicyStore::open(&data_dir.join("policies"))?);
    let sessions = Arc::new(SessionRegistry::open(&data_dir.join("sessions"))?);
    let state = AppState::new(
        auth,
        policies,
        sessions,
        Arc::new(Metrics::default()),
        Arc::new(StreamManager::default()),
    );

    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    let local_addr: SocketAddr = listener.local_addr()?;

    if let Some(ready_path) = &config.ready_file {
        write_ready_file(ready_path, local_addr)?;
    }

    info!(%local_addr, "gaze sidecar listening");
    let app = build_router(state);
    axum::serve(listener, app).await?;
    Ok(())
}

fn write_ready_file(path: &Path, addr: SocketAddr) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let tmp = path.with_extension(format!(
        "tmp-{}",
        std::process::id()
    ));
    let payload = serde_json::json!({
        "address": addr.to_string(),
        "protocol_version": PROTOCOL_VERSION,
    });
    std::fs::write(&tmp, serde_json::to_vec_pretty(&payload).expect("json"))?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
