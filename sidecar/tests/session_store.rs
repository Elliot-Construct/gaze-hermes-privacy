use std::path::{Path, PathBuf};

use gaze_hermes_sidecar::sessions::{SessionKey, SessionRegistry, StoreError};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gaze-hermes-sessions-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn key(profile: &str, session: &str) -> SessionKey {
    SessionKey {
        profile_id: profile.to_string(),
        session_id: session.to_string(),
    }
}

fn snapshot_path_for(dir: &Path, profile: &str, session: &str) -> PathBuf {
    gaze_hermes_sidecar::sessions::snapshot_path(dir, &key(profile, session))
}

#[test]
fn round_trip_restore_returns_plaintext() {
    let dir = temp_dir("round-trip");
    let registry = SessionRegistry::open(&dir).unwrap();
    registry
        .persist_marker(&key("profile-a", "session-1"), b"alice@example.invalid")
        .unwrap();
    let bytes = registry
        .restore_marker(&key("profile-a", "session-1"))
        .unwrap();
    assert_eq!(bytes, b"alice@example.invalid");
}

#[tokio::test]
async fn persist_writes_non_secret_index_and_list_reports_metadata() {
    let dir = temp_dir("index");
    let registry = SessionRegistry::open(&dir).unwrap();
    let k = key("profile-a", "session-1");
    registry.persist_marker(&k, b"alice@example.invalid").unwrap();

    let index_raw = std::fs::read_to_string(dir.join("index.json")).unwrap();
    assert!(index_raw.contains("profile-a"));
    assert!(index_raw.contains("session-1"));
    assert!(!index_raw.contains("alice@example.invalid"));

    let listed = registry.list().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].profile_id, "profile-a");
    assert_eq!(listed[0].session_id, "session-1");
    assert!(listed[0].has_snapshot);
    assert!(!listed[0].recovery_blocked);

    let reopened = SessionRegistry::open(&dir).unwrap();
    let listed = reopened.list().await.unwrap();
    assert_eq!(listed.len(), 1);
}

#[test]
fn wrong_master_key_fails_to_decrypt() {
    let dir = temp_dir("wrong-key");
    let registry = SessionRegistry::open(&dir).unwrap();
    registry
        .persist_marker(&key("profile-a", "session-1"), b"alice@example.invalid")
        .unwrap();

    let mut wrong = [0x42u8; 32];
    wrong[0] ^= 0xff;
    let other = SessionRegistry::open_with_master_key(&dir, wrong).unwrap();
    let err = other
        .restore_marker(&key("profile-a", "session-1"))
        .unwrap_err();
    assert!(matches!(
        err,
        StoreError::Decrypt | StoreError::Integrity
    ));
}

#[tokio::test]
async fn tampered_snapshot_never_replaces_live_session() {
    let dir = temp_dir("tamper");
    let registry = SessionRegistry::open(&dir).unwrap();
    let k = key("profile-a", "session-1");
    registry.persist_marker(&k, b"alice@example.invalid").unwrap();

    let path = snapshot_path_for(&dir, "profile-a", "session-1");
    let mut ciphertext = std::fs::read(&path).unwrap();
    let last = ciphertext.len() - 1;
    ciphertext[last] ^= 0xff;
    std::fs::write(&path, &ciphertext).unwrap();

    let err = registry.restore_marker(&k).unwrap_err();
    assert!(matches!(err, StoreError::Decrypt | StoreError::Integrity));
    let err = registry.get_or_restore(&k).await.unwrap_err();
    assert!(matches!(err, StoreError::RecoveryBlocked));
}

#[test]
fn stale_tmp_ignored_next_to_valid_snapshot() {
    let dir = temp_dir("stale-tmp");
    let registry = SessionRegistry::open(&dir).unwrap();
    let k = key("profile-a", "session-1");
    registry.persist_marker(&k, b"alice@example.invalid").unwrap();

    let path = snapshot_path_for(&dir, "profile-a", "session-1");
    let stale = path.with_extension("tmp-deadbeef");
    std::fs::write(&stale, b"garbage").unwrap();

    let bytes = registry.restore_marker(&k).unwrap();
    assert_eq!(bytes, b"alice@example.invalid");
    assert!(stale.exists());
}

#[test]
fn simulated_failure_before_rename_keeps_existing_snapshot() {
    let dir = temp_dir("fail-before-rename");
    let registry = SessionRegistry::open(&dir).unwrap();
    let k = key("profile-a", "session-1");
    registry.persist_marker(&k, b"first@example.invalid").unwrap();

    registry.set_fail_before_rename(true);
    let err = registry
        .persist_marker(&k, b"second@example.invalid")
        .unwrap_err();
    assert!(matches!(err, StoreError::Io(_)));
    registry.set_fail_before_rename(false);

    let bytes = registry.restore_marker(&k).unwrap();
    assert_eq!(bytes, b"first@example.invalid");
}

#[test]
fn profile_and_session_namespaces_are_separate() {
    let dir = temp_dir("namespaces");
    let registry = SessionRegistry::open(&dir).unwrap();
    registry
        .persist_marker(&key("profile-a", "session-1"), b"a1@example.invalid")
        .unwrap();
    registry
        .persist_marker(&key("profile-a", "session-2"), b"a2@example.invalid")
        .unwrap();
    registry
        .persist_marker(&key("profile-b", "session-1"), b"b1@example.invalid")
        .unwrap();

    let a1 = registry.restore_marker(&key("profile-a", "session-1")).unwrap();
    let a2 = registry.restore_marker(&key("profile-a", "session-2")).unwrap();
    let b1 = registry.restore_marker(&key("profile-b", "session-1")).unwrap();
    assert_eq!(a1, b"a1@example.invalid");
    assert_eq!(a2, b"a2@example.invalid");
    assert_eq!(b1, b"b1@example.invalid");

    let path_a1 = snapshot_path_for(&dir, "profile-a", "session-1");
    let path_b1 = snapshot_path_for(&dir, "profile-b", "session-1");
    assert_ne!(path_a1, path_b1);
}

#[tokio::test]
async fn recovery_blocked_on_corrupt_snapshot_and_delete_clears() {
    let dir = temp_dir("recovery-blocked");
    let registry = SessionRegistry::open(&dir).unwrap();
    let k = key("profile-a", "session-1");
    registry.persist_marker(&k, b"alice@example.invalid").unwrap();

    let path = snapshot_path_for(&dir, "profile-a", "session-1");
    std::fs::write(&path, b"not-a-valid-envelope").unwrap();

    let err = registry.get_or_restore(&k).await.unwrap_err();
    assert!(matches!(err, StoreError::RecoveryBlocked));
    let metadata = registry.metadata(&k).await.unwrap();
    assert!(metadata.recovery_blocked);

    registry.delete(&k).await.unwrap();
    assert!(!path.exists());
    let index_raw = std::fs::read_to_string(dir.join("index.json")).unwrap();
    assert!(!index_raw.contains("session-1"));

    let handle = registry.get_or_restore(&k).await.unwrap();
    assert!(handle.session.try_lock().is_ok());
}

#[test]
fn request_id_not_in_snapshot_path() {
    let dir = temp_dir("no-request-id");
    let registry = SessionRegistry::open(&dir).unwrap();
    let k = key("profile-a", "session-1");
    registry.persist_marker(&k, b"alice@example.invalid").unwrap();

    let entries: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    for name in entries {
        assert!(!name.contains("request"), "unexpected name: {name}");
        assert!(!name.contains("alice"), "plaintext leaked: {name}");
    }
}

#[test]
fn snapshot_bytes_are_not_plaintext() {
    let dir = temp_dir("no-plaintext");
    let registry = SessionRegistry::open(&dir).unwrap();
    let k = key("profile-a", "session-1");
    registry.persist_marker(&k, b"alice@example.invalid").unwrap();
    let path = snapshot_path_for(&dir, "profile-a", "session-1");
    let on_disk = std::fs::read(&path).unwrap();
    assert!(!on_disk.windows(14).any(|w| w == b"alice@example"));
}

#[tokio::test]
async fn get_or_restore_round_trips_gaze_session() {
    let dir = temp_dir("session-round-trip");
    let registry = SessionRegistry::open(&dir).unwrap();
    let k = key("profile-a", "session-1");

    let handle = registry.get_or_restore(&k).await.unwrap();
    {
        let mut session = handle.session.lock().await;
        let _ = &mut session;
    }
    registry.persist(&k).await.unwrap();

    let reloaded = registry.get_or_restore(&k).await.unwrap();
    assert!(reloaded.session.try_lock().is_ok());
}

#[test]
fn open_creates_master_key_once() {
    let dir = temp_dir("master-key");
    let _a = SessionRegistry::open(&dir).unwrap();
    let key_path = dir.join("master.key");
    assert!(key_path.exists());
    let first = std::fs::read(&key_path).unwrap();
    assert_eq!(first.len(), 32);
    let _b = SessionRegistry::open(&dir).unwrap();
    let second = std::fs::read(&key_path).unwrap();
    assert_eq!(first, second);
}

#[test]
fn fail_before_rename_flag_is_atomic() {
    let dir = temp_dir("fail-flag");
    let registry = SessionRegistry::open(&dir).unwrap();
    assert!(!registry.fail_before_rename());
    registry.set_fail_before_rename(true);
    assert!(registry.fail_before_rename());
    registry.set_fail_before_rename(false);
    assert!(!registry.fail_before_rename());
}
