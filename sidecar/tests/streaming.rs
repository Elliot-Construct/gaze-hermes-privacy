use std::path::PathBuf;
use std::sync::Arc;

use gaze_hermes_sidecar::policies::PolicyStore;
use gaze_hermes_sidecar::streaming::{StreamError, StreamManager, StreamRestorer};

const EMAIL: &str = "alice@example.invalid";

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gaze-streaming-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join("policies")).unwrap();
    std::fs::write(
        dir.join("policies").join("global.toml"),
        gaze_hermes_sidecar::config::DEFAULT_GLOBAL_POLICY,
    )
    .unwrap();
    dir
}

fn fixture_session_with_email(
    email: &str,
) -> (Arc<gaze_hermes_sidecar::sessions::SessionHandle>, String) {
    let dir = temp_dir("fixture");
    let store = Arc::new(PolicyStore::open(&dir.join("policies")).unwrap());
    let effective = store.effective(None).unwrap();
    let locale_tags = effective.policy.locale.clone().unwrap_or_default();
    let dictionaries = gaze::DictionaryBundle::default();
    let session = gaze::Session::new(gaze::Scope::Conversation("stream-fixture".into())).unwrap();
    let mut tx = session.begin_transaction();
    let protected = effective
        .pipeline
        .protect_text_transaction(
            &mut tx,
            email,
            gaze::ProtectionContext::strict(&locale_tags, &dictionaries),
        )
        .unwrap();
    tx.commit().unwrap();
    assert_ne!(protected, email, "fixture email must tokenize");
    let handle = Arc::new(gaze_hermes_sidecar::sessions::SessionHandle {
        session: std::sync::Mutex::new(session),
    });
    (handle, protected)
}

#[test]
fn split_token_is_never_emitted_partially() {
    let (session, token) = fixture_session_with_email(EMAIL);
    for split in 1..token.len() {
        if !token.is_char_boundary(split) {
            continue;
        }
        let mut r = StreamRestorer::new(session.clone());
        assert_eq!(r.feed(1, "text", &token[..split]).unwrap(), "");
        assert_eq!(
            r.feed(2, "text", &token[split..]).unwrap(),
            EMAIL,
            "split at byte {split}"
        );
    }
}

#[test]
fn two_tokens_in_one_chunk_restore_together() {
    let (session, token) = fixture_session_with_email(EMAIL);
    let chunk = format!("A {token} and {token} B");
    let mut r = StreamRestorer::new(session.clone());
    let out = r.feed(1, "text", &chunk).unwrap();
    assert_eq!(out, format!("A {EMAIL} and {EMAIL} B"));
}

#[test]
fn interleaved_lanes_keep_independent_carries() {
    let (session, token) = fixture_session_with_email(EMAIL);
    let mut r = StreamRestorer::new(session.clone());
    let mid = token.len() / 2;
    let mid = if token.is_char_boundary(mid) {
        mid
    } else {
        mid + 1
    };
    assert_eq!(r.feed(1, "text", &token[..mid]).unwrap(), "");
    assert_eq!(
        r.feed(2, "reasoning", "thinking...").unwrap(),
        "thinking..."
    );
    assert_eq!(r.feed(3, "text", &token[mid..]).unwrap(), EMAIL);
    r.finish().unwrap();
}

#[test]
fn duplicate_sequence_rejected() {
    let (session, _token) = fixture_session_with_email(EMAIL);
    let mut r = StreamRestorer::new(session.clone());
    r.feed(1, "text", "hello").unwrap();
    let err = r.feed(1, "text", "world").unwrap_err();
    assert!(matches!(err, StreamError::DuplicateSequence));
}

#[test]
fn skipped_sequence_rejected() {
    let (session, _token) = fixture_session_with_email(EMAIL);
    let mut r = StreamRestorer::new(session.clone());
    r.feed(1, "text", "hello").unwrap();
    let err = r.feed(3, "text", "world").unwrap_err();
    assert!(matches!(err, StreamError::SkippedSequence));
}

#[test]
fn finish_with_dangling_token_prefix_fails() {
    let (session, token) = fixture_session_with_email(EMAIL);
    let mut r = StreamRestorer::new(session.clone());
    let mid = token.len() / 2;
    let mid = if token.is_char_boundary(mid) {
        mid
    } else {
        mid + 1
    };
    r.feed(1, "text", &token[..mid]).unwrap();
    let err = r.finish().unwrap_err();
    assert!(matches!(err, StreamError::StrictRestore));
}

#[test]
fn finish_with_complete_text_succeeds() {
    let (session, token) = fixture_session_with_email(EMAIL);
    let mut r = StreamRestorer::new(session.clone());
    r.feed(1, "text", &format!("hi {token}")).unwrap();
    r.finish().unwrap();
}

#[test]
fn abort_removes_stream_state() {
    let (session, token) = fixture_session_with_email(EMAIL);
    let manager = StreamManager::default();
    manager
        .open("s-1", StreamRestorer::new(session.clone()))
        .unwrap();
    assert!(manager.contains("s-1"));
    manager
        .get("s-1")
        .unwrap()
        .feed(1, "text", &token[..1.min(token.len())])
        .unwrap();
    manager.abort("s-1").unwrap();
    assert!(!manager.contains("s-1"));
    let err = manager.abort("s-1").unwrap_err();
    assert!(matches!(err, StreamError::UnknownStream));
}
