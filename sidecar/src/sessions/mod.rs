//! Session registry with encrypted on-disk snapshots.

pub mod store;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use zeroize::Zeroizing;

pub use store::StoreError;

pub const AAD_PREFIX: &[u8] = b"gaze-hermes-session-v1\0";

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SessionKey {
    pub profile_id: String,
    pub session_id: String,
}

impl SessionKey {
    pub fn new(profile_id: impl Into<String>, session_id: impl Into<String>) -> Self {
        Self {
            profile_id: profile_id.into(),
            session_id: session_id.into(),
        }
    }
}

pub struct SessionHandle {
    pub session: Mutex<gaze::Session>,
}

impl std::fmt::Debug for SessionHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionHandle").finish_non_exhaustive()
    }
}

pub struct SessionRegistry {
    dir: PathBuf,
    master_key: Zeroizing<[u8; 32]>,
    handles: tokio::sync::Mutex<std::collections::HashMap<SessionKey, Arc<SessionHandle>>>,
    fail_before_rename: AtomicBool,
}

impl SessionRegistry {
    pub fn open(dir: &Path) -> Result<Self, StoreError> {
        std::fs::create_dir_all(dir)?;
        let master_key = crate::crypto::load_or_create_master_key(&dir.join("master.key"))?;
        Ok(Self {
            dir: dir.to_path_buf(),
            master_key,
            handles: tokio::sync::Mutex::new(std::collections::HashMap::new()),
            fail_before_rename: AtomicBool::new(false),
        })
    }

    pub fn open_with_master_key(dir: &Path, master_key: [u8; 32]) -> Result<Self, StoreError> {
        std::fs::create_dir_all(dir)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            master_key: Zeroizing::new(master_key),
            handles: tokio::sync::Mutex::new(std::collections::HashMap::new()),
            fail_before_rename: AtomicBool::new(false),
        })
    }

    pub fn set_fail_before_rename(&self, fail: bool) {
        self.fail_before_rename.store(fail, Ordering::SeqCst);
    }

    pub fn fail_before_rename(&self) -> bool {
        self.fail_before_rename.load(Ordering::SeqCst)
    }

    pub async fn get_or_restore(
        &self,
        key: &SessionKey,
    ) -> Result<Arc<SessionHandle>, StoreError> {
        {
            let handles = self.handles.lock().await;
            if let Some(handle) = handles.get(key) {
                return Ok(Arc::clone(handle));
            }
        }

        let path = snapshot_path(&self.dir, key);
        let session = match std::fs::read(&path) {
            Ok(ciphertext) => {
                let plaintext = crate::crypto::decrypt_session_payload(
                    &self.master_key,
                    &key.profile_id,
                    &key.session_id,
                    &ciphertext,
                )
                .map_err(|_| StoreError::RecoveryBlocked)?;
                gaze::Session::import(gaze::SensitiveSnapshot::from(plaintext))
                    .map_err(|_| StoreError::RecoveryBlocked)?
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                clean_stale_tmp(&self.dir);
                gaze::Session::new(gaze::Scope::Conversation(key.session_id.clone()))
                    .map_err(|err| StoreError::Session(err.to_string()))?
            }
            Err(err) => return Err(StoreError::Io(err)),
        };

        let handle = Arc::new(SessionHandle {
            session: Mutex::new(session),
        });
        let mut handles = self.handles.lock().await;
        if let Some(existing) = handles.get(key) {
            return Ok(Arc::clone(existing));
        }
        handles.insert(key.clone(), Arc::clone(&handle));
        Ok(handle)
    }

    pub async fn persist(&self, key: &SessionKey) -> Result<(), StoreError> {
        let handle = {
            let handles = self.handles.lock().await;
            handles
                .get(key)
                .cloned()
                .ok_or_else(|| StoreError::Session("no active session".into()))?
        };
        let plaintext = {
            let session = handle.session.lock().await;
            session
                .export()
                .map_err(|err| StoreError::Session(err.to_string()))?
                .into_bytes()
        };
        let envelope = crate::crypto::encrypt_session_payload(
            &self.master_key,
            &key.profile_id,
            &key.session_id,
            &plaintext,
        )?;
        persist_bytes(
            &snapshot_path(&self.dir, key),
            &envelope,
            self.fail_before_rename(),
        )
    }

    pub async fn delete(&self, key: &SessionKey) -> Result<(), StoreError> {
        self.handles.lock().await.remove(key);
        let path = snapshot_path(&self.dir, key);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(StoreError::Io(err)),
        }
    }

    pub fn persist_marker(&self, key: &SessionKey, plaintext: &[u8]) -> Result<(), StoreError> {
        let envelope = crate::crypto::encrypt_session_payload(
            &self.master_key,
            &key.profile_id,
            &key.session_id,
            plaintext,
        )?;
        persist_bytes(
            &snapshot_path(&self.dir, key),
            &envelope,
            self.fail_before_rename(),
        )
    }

    pub fn restore_marker(&self, key: &SessionKey) -> Result<Vec<u8>, StoreError> {
        let path = snapshot_path(&self.dir, key);
        let ciphertext = std::fs::read(&path)?;
        crate::crypto::decrypt_session_payload(
            &self.master_key,
            &key.profile_id,
            &key.session_id,
            &ciphertext,
        )
    }
}

pub fn namespace_bytes(profile_id: &str, session_id: &str) -> Vec<u8> {
    let mut out = AAD_PREFIX.to_vec();
    out.extend_from_slice(profile_id.as_bytes());
    out.push(0);
    out.extend_from_slice(session_id.as_bytes());
    out
}

pub fn snapshot_path(dir: &Path, key: &SessionKey) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(namespace_bytes(&key.profile_id, &key.session_id));
    let digest = hasher.finalize();
    dir.join(format!("{}.enc", hex::encode(digest)))
}

pub(crate) fn persist_bytes(
    path: &Path,
    bytes: &[u8],
    fail_before_rename: bool,
) -> Result<(), StoreError> {
    let file_stem = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("snapshot");
    let tmp = path.with_file_name(format!(
        "{file_stem}.tmp-{}",
        hex::encode(&rand::random::<[u8; 4]>())
    ));
    std::fs::write(&tmp, bytes)?;
    {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().write(true).open(&tmp)?;
        file.sync_all()?;
        file.write_all(b"")?;
        file.sync_all()?;
    }
    if fail_before_rename {
        let _ = std::fs::remove_file(&tmp);
        return Err(StoreError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            "simulated failure before rename",
        )));
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn clean_stale_tmp(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.contains(".tmp-") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}
