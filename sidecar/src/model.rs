//! NER model provisioning behind a trait.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("model install failed: {0}")]
    Install(String),
}

pub trait ModelProvisioner: Send + Sync {
    fn ensure(&self) -> Result<PathBuf, ModelError>;
}

pub struct GazeModelProvisioner {
    pub model_dir: Option<PathBuf>,
}

impl ModelProvisioner for GazeModelProvisioner {
    fn ensure(&self) -> Result<PathBuf, ModelError> {
        use gaze_model_setup::{InstallOptions, InstallOutcome, KijiDistilbertPrecision};
        let options = InstallOptions {
            model_dir: self.model_dir.clone(),
            precision: KijiDistilbertPrecision::Fp32,
        };
        match gaze_model_setup::install_kiji_bundle(&options) {
            Ok(InstallOutcome::AlreadyPresent { model_dir })
            | Ok(InstallOutcome::Installed { model_dir }) => Ok(model_dir),
            Err(err) => Err(ModelError::Install(err.to_string())),
        }
    }
}
