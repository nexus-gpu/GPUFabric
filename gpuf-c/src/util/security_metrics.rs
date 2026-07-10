#[cfg(not(target_os = "ios"))]
use crate::handle::session_cache::{
    worker_session_cache_metrics_snapshot, WorkerSessionCacheMetricsSnapshot,
};
use once_cell::sync::Lazy;
#[cfg(target_os = "ios")]
use serde::Serialize;
#[cfg(not(target_os = "ios"))]
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(target_os = "ios")]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct WorkerSessionCacheMetricsSnapshot {
    pub decisions_total: u64,
    pub session_task_total: u64,
    pub cold_total: u64,
    pub bypass_total: u64,
    pub reset_total: u64,
    pub unsupported_policy_total: u64,
    pub disabled_total: u64,
    pub kv_hit_total: u64,
    pub state_checkpoint_hit_total: u64,
    pub state_checkpoint_miss_total: u64,
    pub state_checkpoint_save_total: u64,
    pub state_checkpoint_reset_total: u64,
    pub state_checkpoint_error_total: u64,
    pub state_checkpoint_quota_eviction_total: u64,
    pub state_checkpoint_bytes_current: u64,
    pub state_checkpoint_max_bytes: u64,
    pub active_sessions_current: usize,
    pub max_sessions: usize,
    pub session_ttl_secs: u64,
    pub metadata_eviction_total: u64,
    pub metadata_stale_total: u64,
}

#[cfg(target_os = "ios")]
fn worker_session_cache_metrics_snapshot() -> WorkerSessionCacheMetricsSnapshot {
    WorkerSessionCacheMetricsSnapshot::default()
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SecurityMetricsSnapshot {
    pub auth_failures: u64,
    pub prompt_rejections: u64,
    pub max_token_rejections: u64,
    pub rate_limit_rejections: u64,
    pub content_filter_rejections: u64,
    pub p2p_auth_rejections: u64,
    pub p2p_replay_rejections: u64,
    pub p2p_reassembly_rejections: u64,
    pub checksum_failures: u64,
    pub public_listen_uses: u64,
    pub external_command_rejections: u64,
    pub worker_session_cache: WorkerSessionCacheMetricsSnapshot,
}

#[derive(Default)]
pub struct SecurityMetrics {
    auth_failures: AtomicU64,
    prompt_rejections: AtomicU64,
    max_token_rejections: AtomicU64,
    rate_limit_rejections: AtomicU64,
    content_filter_rejections: AtomicU64,
    p2p_auth_rejections: AtomicU64,
    p2p_replay_rejections: AtomicU64,
    p2p_reassembly_rejections: AtomicU64,
    checksum_failures: AtomicU64,
    public_listen_uses: AtomicU64,
    external_command_rejections: AtomicU64,
}

static SECURITY_METRICS: Lazy<SecurityMetrics> = Lazy::new(SecurityMetrics::default);

fn inc(counter: &AtomicU64) {
    counter.fetch_add(1, Ordering::Relaxed);
}

pub fn record_auth_failure() {
    inc(&SECURITY_METRICS.auth_failures);
}

pub fn record_prompt_rejection() {
    inc(&SECURITY_METRICS.prompt_rejections);
}

pub fn record_max_token_rejection() {
    inc(&SECURITY_METRICS.max_token_rejections);
}

pub fn record_rate_limit_rejection() {
    inc(&SECURITY_METRICS.rate_limit_rejections);
}

pub fn record_content_filter_rejection() {
    inc(&SECURITY_METRICS.content_filter_rejections);
}

pub fn record_p2p_auth_rejection() {
    inc(&SECURITY_METRICS.p2p_auth_rejections);
}

pub fn record_p2p_replay_rejection() {
    inc(&SECURITY_METRICS.p2p_replay_rejections);
}

pub fn record_p2p_reassembly_rejection() {
    inc(&SECURITY_METRICS.p2p_reassembly_rejections);
}

pub fn record_checksum_failure() {
    inc(&SECURITY_METRICS.checksum_failures);
}

pub fn record_public_listen_use() {
    inc(&SECURITY_METRICS.public_listen_uses);
}

pub fn record_external_command_rejection() {
    inc(&SECURITY_METRICS.external_command_rejections);
}

pub fn snapshot() -> SecurityMetricsSnapshot {
    SecurityMetricsSnapshot {
        auth_failures: SECURITY_METRICS.auth_failures.load(Ordering::Relaxed),
        prompt_rejections: SECURITY_METRICS.prompt_rejections.load(Ordering::Relaxed),
        max_token_rejections: SECURITY_METRICS
            .max_token_rejections
            .load(Ordering::Relaxed),
        rate_limit_rejections: SECURITY_METRICS
            .rate_limit_rejections
            .load(Ordering::Relaxed),
        content_filter_rejections: SECURITY_METRICS
            .content_filter_rejections
            .load(Ordering::Relaxed),
        p2p_auth_rejections: SECURITY_METRICS.p2p_auth_rejections.load(Ordering::Relaxed),
        p2p_replay_rejections: SECURITY_METRICS
            .p2p_replay_rejections
            .load(Ordering::Relaxed),
        p2p_reassembly_rejections: SECURITY_METRICS
            .p2p_reassembly_rejections
            .load(Ordering::Relaxed),
        checksum_failures: SECURITY_METRICS.checksum_failures.load(Ordering::Relaxed),
        public_listen_uses: SECURITY_METRICS.public_listen_uses.load(Ordering::Relaxed),
        external_command_rejections: SECURITY_METRICS
            .external_command_rejections
            .load(Ordering::Relaxed),
        worker_session_cache: worker_session_cache_metrics_snapshot(),
    }
}

#[cfg(test)]
pub fn reset_for_tests() {
    SECURITY_METRICS.auth_failures.store(0, Ordering::Relaxed);
    SECURITY_METRICS
        .prompt_rejections
        .store(0, Ordering::Relaxed);
    SECURITY_METRICS
        .max_token_rejections
        .store(0, Ordering::Relaxed);
    SECURITY_METRICS
        .rate_limit_rejections
        .store(0, Ordering::Relaxed);
    SECURITY_METRICS
        .content_filter_rejections
        .store(0, Ordering::Relaxed);
    SECURITY_METRICS
        .p2p_auth_rejections
        .store(0, Ordering::Relaxed);
    SECURITY_METRICS
        .p2p_replay_rejections
        .store(0, Ordering::Relaxed);
    SECURITY_METRICS
        .p2p_reassembly_rejections
        .store(0, Ordering::Relaxed);
    SECURITY_METRICS
        .checksum_failures
        .store(0, Ordering::Relaxed);
    SECURITY_METRICS
        .public_listen_uses
        .store(0, Ordering::Relaxed);
    SECURITY_METRICS
        .external_command_rejections
        .store(0, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handle::session_cache::{
        record_worker_cache_decision, record_worker_state_checkpoint_quota_eviction,
        reset_worker_session_cache_metrics_for_tests, set_worker_state_checkpoint_bytes_current,
        set_worker_state_checkpoint_max_bytes, worker_session_cache_test_guard,
    };

    #[test]
    fn records_metrics() {
        let before = snapshot();
        record_auth_failure();
        record_p2p_auth_rejection();
        record_checksum_failure();

        let after = snapshot();
        assert!(after.auth_failures >= before.auth_failures + 1);
        assert!(after.p2p_auth_rejections >= before.p2p_auth_rejections + 1);
        assert!(after.checksum_failures >= before.checksum_failures + 1);
    }

    #[test]
    fn snapshot_includes_worker_session_cache_metrics() {
        let _guard = worker_session_cache_test_guard();
        reset_worker_session_cache_metrics_for_tests();
        record_worker_cache_decision(Some("session-1234567890"), Some("reset"));

        let metrics = snapshot();
        assert_eq!(metrics.worker_session_cache.decisions_total, 1);
        assert_eq!(metrics.worker_session_cache.session_task_total, 1);
        assert_eq!(metrics.worker_session_cache.reset_total, 1);
        assert_eq!(metrics.worker_session_cache.kv_hit_total, 0);
        assert_eq!(metrics.worker_session_cache.active_sessions_current, 0);
    }

    #[test]
    fn snapshot_includes_worker_state_checkpoint_quota_metrics() {
        let _guard = worker_session_cache_test_guard();
        reset_worker_session_cache_metrics_for_tests();
        record_worker_state_checkpoint_quota_eviction(3);
        set_worker_state_checkpoint_bytes_current(4096);
        set_worker_state_checkpoint_max_bytes(8192);

        let metrics = snapshot();
        assert_eq!(
            metrics
                .worker_session_cache
                .state_checkpoint_quota_eviction_total,
            3
        );
        assert_eq!(
            metrics.worker_session_cache.state_checkpoint_bytes_current,
            4096
        );
        assert_eq!(
            metrics.worker_session_cache.state_checkpoint_max_bytes,
            8192
        );
    }
}
