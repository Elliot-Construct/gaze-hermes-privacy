use std::collections::HashMap;
use std::sync::Arc;

use crate::sessions::SessionHandle;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StreamError {
    #[error("duplicate sequence")]
    DuplicateSequence,
    #[error("skipped sequence")]
    SkippedSequence,
    #[error("strict restore failed")]
    StrictRestore,
    #[error("unknown stream")]
    UnknownStream,
    #[error("stream already open")]
    AlreadyOpen,
}

impl StreamError {
    pub fn code(&self) -> &'static str {
        match self {
            StreamError::DuplicateSequence => "duplicate_sequence",
            StreamError::SkippedSequence => "skipped_sequence",
            StreamError::StrictRestore => "strict_restore_failed",
            StreamError::UnknownStream => "unknown_stream",
            StreamError::AlreadyOpen => "stream_already_open",
        }
    }
}

pub struct StreamRestorer {
    handle: Arc<SessionHandle>,
    tokens: Vec<String>,
    lanes: HashMap<String, String>,
    next_seq: u64,
    finished: bool,
}

impl StreamRestorer {
    pub fn new(handle: Arc<SessionHandle>) -> Self {
        let tokens = {
            let session = handle.session.lock().expect("session lock");
            session.tokens().to_vec()
        };
        Self {
            handle,
            tokens,
            lanes: HashMap::new(),
            next_seq: 1,
            finished: false,
        }
    }

    pub fn feed(&mut self, seq: u64, kind: &str, chunk: &str) -> Result<String, StreamError> {
        if self.finished {
            return Err(StreamError::SkippedSequence);
        }
        if seq == self.next_seq {
            self.next_seq += 1;
        } else if seq < self.next_seq {
            return Err(StreamError::DuplicateSequence);
        } else {
            return Err(StreamError::SkippedSequence);
        }
        let carry = self.lanes.entry(kind.to_string()).or_default();
        feed_lane(&self.handle, &self.tokens, carry, chunk)
    }

    pub fn finish(&mut self) -> Result<(), StreamError> {
        if self.finished {
            return Ok(());
        }
        for carry in self.lanes.values_mut() {
            if carry.is_empty() {
                continue;
            }
            let pending = std::mem::take(carry);
            if longest_owned_token_prefix_suffix(&pending, &self.tokens) == pending.len() {
                return Err(StreamError::StrictRestore);
            }
            restore(&self.handle, &pending)?;
        }
        self.lanes.clear();
        self.finished = true;
        Ok(())
    }
}

fn restore(handle: &SessionHandle, text: &str) -> Result<String, StreamError> {
    let session = handle.session.lock().expect("session lock");
    session
        .restore_strict_text(text)
        .map_err(|_| StreamError::StrictRestore)
}

fn longest_owned_token_prefix_suffix(text: &str, tokens: &[String]) -> usize {
    let mut starts: Vec<usize> = text.char_indices().map(|(index, _)| index).collect();
    starts.push(text.len());
    for start in starts.into_iter().rev() {
        let suffix = &text[start..];
        if !suffix.is_empty()
            && tokens
                .iter()
                .any(|token| token.starts_with(suffix) && token.len() > suffix.len())
        {
            return suffix.len();
        }
    }
    0
}

fn feed_lane(
    handle: &SessionHandle,
    tokens: &[String],
    carry: &mut String,
    chunk: &str,
) -> Result<String, StreamError> {
    carry.push_str(chunk);
    let held = longest_owned_token_prefix_suffix(carry, tokens);
    let safe_len = carry.len() - held;
    let safe = carry[..safe_len].to_string();
    let pending = carry[safe_len..].to_string();
    *carry = pending;
    restore(handle, &safe)
}

#[derive(Default)]
pub struct StreamManager {
    streams: std::sync::Mutex<HashMap<String, StreamRestorer>>,
}

impl StreamManager {
    pub fn open(&self, stream_id: &str, restorer: StreamRestorer) -> Result<(), StreamError> {
        let mut streams = self.streams.lock().expect("stream manager poisoned");
        if streams.contains_key(stream_id) {
            return Err(StreamError::AlreadyOpen);
        }
        streams.insert(stream_id.to_string(), restorer);
        Ok(())
    }

    pub fn contains(&self, stream_id: &str) -> bool {
        self.streams
            .lock()
            .expect("stream manager poisoned")
            .contains_key(stream_id)
    }

    pub fn get<'a>(&'a self, stream_id: &'a str) -> Result<StreamGuard<'a>, StreamError> {
        if self.contains(stream_id) {
            Ok(StreamGuard {
                streams: &self.streams,
                stream_id,
            })
        } else {
            Err(StreamError::UnknownStream)
        }
    }

    pub fn abort(&self, stream_id: &str) -> Result<(), StreamError> {
        let removed = self
            .streams
            .lock()
            .expect("stream manager poisoned")
            .remove(stream_id);
        if removed.is_some() {
            Ok(())
        } else {
            Err(StreamError::UnknownStream)
        }
    }
}

pub struct StreamGuard<'a> {
    streams: &'a std::sync::Mutex<HashMap<String, StreamRestorer>>,
    stream_id: &'a str,
}

impl StreamGuard<'_> {
    pub fn feed(&self, seq: u64, kind: &str, chunk: &str) -> Result<String, StreamError> {
        let mut streams = self.streams.lock().expect("stream manager poisoned");
        let restorer = streams
            .get_mut(self.stream_id)
            .ok_or(StreamError::UnknownStream)?;
        restorer.feed(seq, kind, chunk)
    }

    pub fn finish(&self) -> Result<(), StreamError> {
        let mut streams = self.streams.lock().expect("stream manager poisoned");
        let restorer = streams
            .get_mut(self.stream_id)
            .ok_or(StreamError::UnknownStream)?;
        restorer.finish()
    }
}
