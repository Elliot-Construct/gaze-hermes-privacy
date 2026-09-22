use std::sync::atomic::{AtomicU64, Ordering};

use axum::extract::State;
use axum::Json;
use serde::Serialize;

use super::AppState;

#[derive(Debug, Default)]
pub struct Metrics {
    pub clean_requests: AtomicU64,
    pub clean_errors: AtomicU64,
    pub restore_requests: AtomicU64,
    pub restore_errors: AtomicU64,
    pub clean_latency_ms_sum: AtomicU64,
    pub clean_latency_ms_max: AtomicU64,
    pub restore_latency_ms_sum: AtomicU64,
    pub restore_latency_ms_max: AtomicU64,
    pub policy_apply_conflicts: AtomicU64,
}

#[derive(Serialize)]
pub struct MetricsSnapshot {
    pub clean_requests: u64,
    pub clean_errors: u64,
    pub restore_requests: u64,
    pub restore_errors: u64,
    pub clean_latency_ms_sum: u64,
    pub clean_latency_ms_max: u64,
    pub restore_latency_ms_sum: u64,
    pub restore_latency_ms_max: u64,
    pub policy_apply_conflicts: u64,
}

impl Metrics {
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            clean_requests: self.clean_requests.load(Ordering::Relaxed),
            clean_errors: self.clean_errors.load(Ordering::Relaxed),
            restore_requests: self.restore_requests.load(Ordering::Relaxed),
            restore_errors: self.restore_errors.load(Ordering::Relaxed),
            clean_latency_ms_sum: self.clean_latency_ms_sum.load(Ordering::Relaxed),
            clean_latency_ms_max: self.clean_latency_ms_max.load(Ordering::Relaxed),
            restore_latency_ms_sum: self.restore_latency_ms_sum.load(Ordering::Relaxed),
            restore_latency_ms_max: self.restore_latency_ms_max.load(Ordering::Relaxed),
            policy_apply_conflicts: self.policy_apply_conflicts.load(Ordering::Relaxed),
        }
    }

    pub fn record_policy_conflict(&self) {
        self.policy_apply_conflicts.fetch_add(1, Ordering::Relaxed);
    }
}

pub async fn metrics_handler(State(state): State<AppState>) -> Json<MetricsSnapshot> {
    Json(state.metrics.snapshot())
}
