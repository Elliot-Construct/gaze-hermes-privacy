//! Process configuration: bind address, token file, readiness rendezvous.

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Parser;

pub const DEFAULT_BIND: &str = "127.0.0.1:65113";

pub const DEFAULT_GLOBAL_POLICY: &str = include_str!("../../policies/default.toml");

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

    /// Data directory for policies and encrypted session snapshots.
    #[arg(long, env = "GAZE_SIDECAR_DATA_DIR")]
    pub data_dir: Option<PathBuf>,

    /// Path to the snapshot master key file. If not provided, a key is generated/stored in the data directory.
    #[arg(long, env = "GAZE_SIDECAR_MASTER_KEY_FILE")]
    pub master_key_file: Option<PathBuf>,

    /// Allow binding to non-loopback addresses (required for Docker).
    #[arg(long, env = "GAZE_SIDECAR_ALLOW_NON_LOOPBACK", default_value_t = false)]
    pub allow_non_loopback: bool,
}

impl Config {
    pub fn data_dir(&self) -> PathBuf {
        if let Some(dir) = &self.data_dir {
            return dir.clone();
        }
        if let Some(parent) = self.api_token_file.parent() {
            if !parent.as_os_str().is_empty() {
                return parent.join("data");
            }
        }
        if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
            if !dir.is_empty() {
                return PathBuf::from(dir).join("gaze-hermes-privacy");
            }
        }
        if let Some(dir) = std::env::var_os("XDG_DATA_HOME") {
            if !dir.is_empty() {
                return PathBuf::from(dir).join("gaze-hermes-privacy");
            }
        }
        if let Some(dir) = std::env::var_os("HOME") {
            if !dir.is_empty() {
                return PathBuf::from(dir)
                    .join(".local")
                    .join("share")
                    .join("gaze-hermes-privacy");
            }
        }
        PathBuf::from("gaze-hermes-privacy-data")
    }

    pub fn bootstrap_data_dir(&self) -> Result<PathBuf, ConfigError> {
        let data_dir = self.data_dir();
        let policies_dir = data_dir.join("policies");
        let profiles_dir = policies_dir.join("profiles");
        std::fs::create_dir_all(&profiles_dir).map_err(|source| ConfigError::DataDir {
            path: profiles_dir.clone(),
            source,
        })?;
        let global = policies_dir.join("global.toml");
        if !global.exists() {
            std::fs::write(&global, DEFAULT_GLOBAL_POLICY).map_err(|source| {
                ConfigError::DataDir {
                    path: global.clone(),
                    source,
                }
            })?;
        }
        std::fs::create_dir_all(data_dir.join("sessions")).map_err(|source| {
            ConfigError::DataDir {
                path: data_dir.clone(),
                source,
            }
        })?;
        Ok(data_dir)
    }

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
        if !is_loopback && !self.allow_non_loopback {
            return Err(ConfigError::BindNotLoopback(self.bind));
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
    #[error("bind address {0} is not loopback (use GAZE_SIDECAR_ALLOW_NON_LOOPBACK to override)")]
    BindNotLoopback(SocketAddr),
    #[error("failed to prepare data directory {path}: {source}")]
    DataDir {
        path: PathBuf,
        source: std::io::Error,
    },
}
