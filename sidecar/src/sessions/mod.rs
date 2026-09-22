//! Session registry with encrypted on-disk snapshots.

pub mod store;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use sha2::{Digest, Sha256};
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
    pub session: std::sync::Mutex<gaze::Session>,
}

impl std::fmt::Debug for SessionHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionHandle").finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct SessionMetadata {
    pub profile_id: String,
    pub session_id: String,
    pub has_snapshot: bool,
    pub recovery_blocked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<std::time::SystemTime>,
    pub mapping_count: usize,
}

pub struct SessionRegistry {
    dir: PathBuf,
    master_key: Zeroizing<[u8; 32]>,
    handles: tokio::sync::Mutex<std::collections::HashMap<SessionKey, Arc<SessionHandle>>>,
    index: std::sync::Mutex<std::collections::HashSet<SessionKey>>,
    blocked: tokio::sync::Mutex<std::collections::HashSet<SessionKey>>,
    fail_before_rename: AtomicBool,
}

fn index_path(dir: &Path) -> PathBuf {
    dir.join("index.json")
}

fn load_index(dir: &Path) -> std::collections::HashSet<SessionKey> {
    let Ok(raw) = std::fs::read_to_string(index_path(dir)) else {
        return std::collections::HashSet::new();
    };
    let Ok(entries) = serde_json::from_str::<Vec<SessionKey>>(&raw) else {
        return std::collections::HashSet::new();
    };
    entries.into_iter().collect()
}

fn write_index(
    dir: &Path,
    index: &std::collections::HashSet<SessionKey>,
) -> Result<(), StoreError> {
    let mut entries: Vec<SessionKey> = index.iter().cloned().collect();
    entries.sort_by(|a, b| (&a.profile_id, &a.session_id).cmp(&(&b.profile_id, &b.session_id)));
    let bytes =
        serde_json::to_vec_pretty(&entries).map_err(|err| StoreError::Session(err.to_string()))?;
    persist_bytes(&index_path(dir), &bytes, false)
}

impl SessionRegistry {
    pub fn open(dir: &Path) -> Result<Self, StoreError> {
        std::fs::create_dir_all(dir)?;
        let master_key = crate::crypto::load_or_create_master_key(&dir.join("master.key"))?;
        Ok(Self {
            dir: dir.to_path_buf(),
            master_key,
            handles: tokio::sync::Mutex::new(std::collections::HashMap::new()),
            index: std::sync::Mutex::new(load_index(dir)),
            blocked: tokio::sync::Mutex::new(std::collections::HashSet::new()),
            fail_before_rename: AtomicBool::new(false),
        })
    }

    pub fn open_with_master_key(dir: &Path, master_key: [u8; 32]) -> Result<Self, StoreError> {
        std::fs::create_dir_all(dir)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            master_key: Zeroizing::new(master_key),
            handles: tokio::sync::Mutex::new(std::collections::HashMap::new()),
            index: std::sync::Mutex::new(load_index(dir)),
            blocked: tokio::sync::Mutex::new(std::collections::HashSet::new()),
            fail_before_rename: AtomicBool::new(false),
        })
    }

    pub fn set_fail_before_rename(&self, fail: bool) {
        self.fail_before_rename.store(fail, Ordering::SeqCst);
    }

    pub fn fail_before_rename(&self) -> bool {
        self.fail_before_rename.load(Ordering::SeqCst)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub async fn get_or_restore(&self, key: &SessionKey) -> Result<Arc<SessionHandle>, StoreError> {
        {
            let handles = self.handles.lock().await;
            if let Some(handle) = handles.get(key) {
                return Ok(Arc::clone(handle));
            }
        }

        let path = snapshot_path(&self.dir, key);
        let session = match std::fs::read(&path) {
            Ok(ciphertext) => {
                let restored = crate::crypto::decrypt_session_payload(
                    &self.master_key,
                    &key.profile_id,
                    &key.session_id,
                    &ciphertext,
                )
                .map_err(|_| StoreError::RecoveryBlocked)
                .and_then(|plaintext| {
                    gaze::Session::import(gaze::SensitiveSnapshot::from(plaintext))
                        .map_err(|_| StoreError::RecoveryBlocked)
                });
                match restored {
                    Ok(session) => {
                        self.blocked.lock().await.remove(key);
                        session
                    }
                    Err(err) => {
                        self.blocked.lock().await.insert(key.clone());
                        return Err(err);
                    }
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                clean_stale_tmp(&self.dir);
                gaze::Session::new(gaze::Scope::Conversation(key.session_id.clone()))
                    .map_err(|err| StoreError::Session(err.to_string()))?
            }
            Err(err) => return Err(StoreError::Io(err)),
        };

        let handle = Arc::new(SessionHandle {
            session: std::sync::Mutex::new(session),
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
            let session = handle.session.lock().expect("session lock");
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
        )?;
        self.record_index(key).await
    }

    pub async fn delete(&self, key: &SessionKey) -> Result<(), StoreError> {
        self.handles.lock().await.remove(key);
        let path = snapshot_path(&self.dir, key);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(StoreError::Io(err)),
        }
        self.blocked.lock().await.remove(key);
        let mut index = self.index.lock().expect("session index poisoned");
        if index.remove(key) {
            write_index(&self.dir, &index)?;
        }
        Ok(())
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
        )?;
        let mut index = self.index.lock().expect("session index poisoned");
        index.insert(key.clone());
        write_index(&self.dir, &index)
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

    async fn record_index(&self, key: &SessionKey) -> Result<(), StoreError> {
        let mut index = self.index.lock().expect("session index poisoned");
        index.insert(key.clone());
        write_index(&self.dir, &index)
    }

    pub async fn list(&self) -> Result<Vec<SessionMetadata>, StoreError> {
        let mut keys: Vec<SessionKey> = self
            .index
            .lock()
            .expect("session index poisoned")
            .iter()
            .cloned()
            .collect();
        keys.sort_by(|a, b| (&a.profile_id, &a.session_id).cmp(&(&b.profile_id, &b.session_id)));
        let mut out = Vec::with_capacity(keys.len());
        for key in keys {
            out.push(self.metadata_inner(&key).await);
        }
        Ok(out)
    }

    pub async fn metadata(&self, key: &SessionKey) -> Result<SessionMetadata, StoreError> {
        let known = self
            .index
            .lock()
            .expect("session index poisoned")
            .contains(key)
            || self.handles.lock().await.contains_key(key);
        if !known {
            return Err(StoreError::Session(format!(
                "unknown session {}/{}",
                key.profile_id, key.session_id
            )));
        }
        Ok(self.metadata_inner(key).await)
    }

    async fn metadata_inner(&self, key: &SessionKey) -> SessionMetadata {
        let path = snapshot_path(&self.dir, key);
        let meta = std::fs::metadata(&path);
        let has_snapshot = meta.is_ok();
        let updated_at = meta.ok().and_then(|m| m.modified().ok());
        let recovery_blocked = self.blocked.lock().await.contains(key);
        let mapping_count = {
            let handles = self.handles.lock().await;
            match handles.get(key) {
                Some(handle) => {
                    let session = handle.session.lock().expect("session lock");
                    session.tokens().len()
                }
                None => 0,
            }
        };
        SessionMetadata {
            profile_id: key.profile_id.clone(),
            session_id: key.session_id.clone(),
            has_snapshot,
            recovery_blocked,
            updated_at,
            mapping_count,
        }
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
    dir.join(snapshot_file_name(key))
}

pub fn snapshot_file_name(key: &SessionKey) -> String {
    let mut hasher = Sha256::new();
    hasher.update(namespace_bytes(&key.profile_id, &key.session_id));
    format!("{}.enc", hex::encode(hasher.finalize()))
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
        hex::encode(rand::random::<[u8; 4]>())
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
        return Err(StoreError::Io(std::io::Error::other(
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
