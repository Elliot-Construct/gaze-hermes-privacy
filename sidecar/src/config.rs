//! Process configuration: bind address, token file, readiness rendezvous.

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Parser;

pub const DEFAULT_BIND: &str = "127.0.0.1:65113";

#[derive(Debug, Parser)]
#[command(name = "gaze-hermes-sidecar", version, about)]
pub struct Config {
    /// Bind address. Use 127.0.0.1:0 only for profile-isolated ephemeral ports.
    #[arg(long, default_value = DEFAULT_BIND, env = "GAZE_SIDECAR_BIND")]
    pub bind: SocketAddr,

    /// Path to the bearer token file. Empty/missing file is a startup error.
    #[arg(long, env = "GAZE_SIDECAR_API_TOKEN_FILE")]
    pub api_token_file: PathBuf,

    /// Optional JSON readiness rendezvous written after bind: {"address","protocol_version"}.
    #[arg(long, env = "GAZE_SIDECAR_READY_FILE")]
    pub ready_file: Option<PathBuf>,
}

impl Config {
    pub fn load_token(&self) -> Result<String, ConfigError> {
        let raw = std::fs::read_to_string(&self.api_token_file).map_err(|source| {
            ConfigError::TokenRead {
                path: self.api_token_file.clone(),
                source,
            }
        })?;
        let token = raw.trim().to_string();
        if token.is_empty() {
            return Err(ConfigError::TokenEmpty {
                path: self.api_token_file.clone(),
            });
        }
        Ok(token)
    }

    pub fn validate_bind(&self) -> Result<(), ConfigError> {
        let is_loopback = matches!(
            self.bind.ip(),
            std::net::IpAddr::V4(ip) if ip.is_loopback()
        ) || matches!(
            self.bind.ip(),
            std::net::IpAddr::V6(ip) if ip.is_loopback()
        );
        if !is_loopback {
            return Err(ConfigError::BindNotLoopback(self.bind));
        }
        if self.bind.port() != 0 && self.bind.port() != 65113 {
            // Non-default fixed ports are allowed only for external operators; still loopback.
            return Ok(());
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read api token file {path}: {source}")]
    TokenRead {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("api token file {path} is empty")]
    TokenEmpty { path: PathBuf },
    #[error("bind address {0} is not loopback")]
    BindNotLoopback(SocketAddr),
}
