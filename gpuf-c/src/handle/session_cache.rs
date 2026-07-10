use once_cell::sync::Lazy;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const DEFAULT_WORKER_SESSION_MAX_ENTRIES: usize = 256;
const DEFAULT_WORKER_SESSION_TTL_SECS: u64 = 30 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerCachePolicy {
    Auto,
    Bypass,
    Reset,
    Unknown,
}

impl WorkerCachePolicy {
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(str::trim).filter(|s| !s.is_empty()) {
            None => Self::Auto,
            Some(value) if value.eq_ignore_ascii_case("auto") => Self::Auto,
            Some(value) if value.eq_ignore_ascii_case("bypass") => Self::Bypass,
            Some(value) if value.eq_ignore_ascii_case("reset") => Self::Reset,
            Some(_) => Self::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Bypass => "bypass",
            Self::Reset => "reset",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum WorkerCacheDecisionStatus {
    Cold,
    Bypass,
    Reset,
    UnsupportedPolicy,
    Disabled,
}

impl WorkerCacheDecisionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Bypass => "bypass",
            Self::Reset => "reset",
            Self::UnsupportedPolicy => "unsupported_policy",
            Self::Disabled => "disabled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerCacheDecision {
    pub session_hash: Option<String>,
    pub policy: WorkerCachePolicy,
    pub status: WorkerCacheDecisionStatus,
    pub kv_reuse_enabled: bool,
}

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

#[derive(Default)]
struct WorkerSessionCacheMetrics {
    decisions_total: AtomicU64,
    session_task_total: AtomicU64,
    cold_total: AtomicU64,
    bypass_total: AtomicU64,
    reset_total: AtomicU64,
    unsupported_policy_total: AtomicU64,
    disabled_total: AtomicU64,
    kv_hit_total: AtomicU64,
    state_checkpoint_hit_total: AtomicU64,
    state_checkpoint_miss_total: AtomicU64,
    state_checkpoint_save_total: AtomicU64,
    state_checkpoint_reset_total: AtomicU64,
    state_checkpoint_error_total: AtomicU64,
    state_checkpoint_quota_eviction_total: AtomicU64,
    state_checkpoint_bytes_current: AtomicU64,
    state_checkpoint_max_bytes: AtomicU64,
    metadata_eviction_total: AtomicU64,
    metadata_stale_total: AtomicU64,
}

static WORKER_SESSION_CACHE_METRICS: Lazy<WorkerSessionCacheMetrics> =
    Lazy::new(WorkerSessionCacheMetrics::default);
static WORKER_SESSION_INDEX: Lazy<Mutex<HashMap<String, WorkerSessionEntry>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone, Copy)]
struct WorkerSessionCacheLimits {
    max_entries: usize,
    ttl: Duration,
}

#[derive(Debug, Clone)]
struct WorkerSessionEntry {
    created_at: Instant,
    last_used: Instant,
}

#[cfg(test)]
static WORKER_SESSION_CACHE_LIMITS_FOR_TESTS: Lazy<Mutex<Option<WorkerSessionCacheLimits>>> =
    Lazy::new(|| Mutex::new(None));

fn inc(counter: &AtomicU64) {
    counter.fetch_add(1, Ordering::Relaxed);
}

fn inc_by(counter: &AtomicU64, value: u64) {
    counter.fetch_add(value, Ordering::Relaxed);
}

fn short_session_hash(session_id: &str) -> String {
    let digest = Sha256::digest(session_id.as_bytes());
    hex::encode(&digest[..6])
}

fn positive_usize_env(name: &str, default_value: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default_value)
}

fn positive_u64_env(name: &str, default_value: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default_value)
}

fn worker_session_cache_limits() -> WorkerSessionCacheLimits {
    #[cfg(test)]
    if let Some(limits) = *WORKER_SESSION_CACHE_LIMITS_FOR_TESTS.lock().unwrap() {
        return limits;
    }

    WorkerSessionCacheLimits {
        max_entries: positive_usize_env(
            "GPUF_WORKER_SESSION_MAX_ENTRIES",
            DEFAULT_WORKER_SESSION_MAX_ENTRIES,
        ),
        ttl: Duration::from_secs(positive_u64_env(
            "GPUF_WORKER_SESSION_TTL_SECS",
            DEFAULT_WORKER_SESSION_TTL_SECS,
        )),
    }
}

fn prune_expired_sessions(
    sessions: &mut HashMap<String, WorkerSessionEntry>,
    now: Instant,
    ttl: Duration,
) {
    let before = sessions.len();
    sessions.retain(|_, entry| now.duration_since(entry.last_used) <= ttl);
    let removed = before.saturating_sub(sessions.len());
    if removed > 0 {
        inc_by(
            &WORKER_SESSION_CACHE_METRICS.metadata_stale_total,
            removed as u64,
        );
    }
}

fn evict_lru_session_if_needed(
    sessions: &mut HashMap<String, WorkerSessionEntry>,
    max_entries: usize,
) {
    if sessions.len() < max_entries {
        return;
    }

    let victim = sessions
        .iter()
        .min_by_key(|(_, entry)| (entry.last_used, entry.created_at))
        .map(|(session_hash, _)| session_hash.clone());

    if let Some(session_hash) = victim {
        sessions.remove(&session_hash);
        inc(&WORKER_SESSION_CACHE_METRICS.metadata_eviction_total);
    }
}

fn update_session_index(
    session_hash: Option<&str>,
    policy: WorkerCachePolicy,
    status: WorkerCacheDecisionStatus,
    now: Instant,
) {
    let Some(session_hash) = session_hash else {
        return;
    };

    let limits = worker_session_cache_limits();
    let mut sessions = WORKER_SESSION_INDEX.lock().unwrap();
    prune_expired_sessions(&mut sessions, now, limits.ttl);

    match (policy, status) {
        (WorkerCachePolicy::Auto, WorkerCacheDecisionStatus::Cold) => {
            if let Some(entry) = sessions.get_mut(session_hash) {
                entry.last_used = now;
                return;
            }

            evict_lru_session_if_needed(&mut sessions, limits.max_entries);
            sessions.insert(
                session_hash.to_string(),
                WorkerSessionEntry {
                    created_at: now,
                    last_used: now,
                },
            );
        }
        (WorkerCachePolicy::Reset, WorkerCacheDecisionStatus::Reset) => {
            sessions.remove(session_hash);
        }
        _ => {}
    }
}

pub fn record_worker_cache_decision(
    session_id: Option<&str>,
    cache_policy: Option<&str>,
) -> WorkerCacheDecision {
    let policy = WorkerCachePolicy::parse(cache_policy);
    let session_hash = session_id.map(short_session_hash);
    let status = if session_hash.is_none() {
        WorkerCacheDecisionStatus::Disabled
    } else {
        match policy {
            WorkerCachePolicy::Auto => WorkerCacheDecisionStatus::Cold,
            WorkerCachePolicy::Bypass => WorkerCacheDecisionStatus::Bypass,
            WorkerCachePolicy::Reset => WorkerCacheDecisionStatus::Reset,
            WorkerCachePolicy::Unknown => WorkerCacheDecisionStatus::UnsupportedPolicy,
        }
    };

    inc(&WORKER_SESSION_CACHE_METRICS.decisions_total);
    if session_hash.is_some() {
        inc(&WORKER_SESSION_CACHE_METRICS.session_task_total);
    }
    match status {
        WorkerCacheDecisionStatus::Cold => inc(&WORKER_SESSION_CACHE_METRICS.cold_total),
        WorkerCacheDecisionStatus::Bypass => inc(&WORKER_SESSION_CACHE_METRICS.bypass_total),
        WorkerCacheDecisionStatus::Reset => inc(&WORKER_SESSION_CACHE_METRICS.reset_total),
        WorkerCacheDecisionStatus::UnsupportedPolicy => {
            inc(&WORKER_SESSION_CACHE_METRICS.unsupported_policy_total)
        }
        WorkerCacheDecisionStatus::Disabled => inc(&WORKER_SESSION_CACHE_METRICS.disabled_total),
    }
    update_session_index(session_hash.as_deref(), policy, status, Instant::now());

    WorkerCacheDecision {
        session_hash,
        policy,
        status,
        kv_reuse_enabled: false,
    }
}

pub fn record_worker_state_checkpoint_hit() {
    inc(&WORKER_SESSION_CACHE_METRICS.state_checkpoint_hit_total);
}

pub fn record_worker_state_checkpoint_miss() {
    inc(&WORKER_SESSION_CACHE_METRICS.state_checkpoint_miss_total);
}

pub fn record_worker_state_checkpoint_save() {
    inc(&WORKER_SESSION_CACHE_METRICS.state_checkpoint_save_total);
}

pub fn record_worker_state_checkpoint_reset() {
    inc(&WORKER_SESSION_CACHE_METRICS.state_checkpoint_reset_total);
}

pub fn record_worker_state_checkpoint_error() {
    inc(&WORKER_SESSION_CACHE_METRICS.state_checkpoint_error_total);
}

pub fn record_worker_state_checkpoint_quota_eviction(count: u64) {
    inc_by(
        &WORKER_SESSION_CACHE_METRICS.state_checkpoint_quota_eviction_total,
        count,
    );
}

pub fn set_worker_state_checkpoint_bytes_current(bytes: u64) {
    WORKER_SESSION_CACHE_METRICS
        .state_checkpoint_bytes_current
        .store(bytes, Ordering::Relaxed);
}

pub fn set_worker_state_checkpoint_max_bytes(bytes: u64) {
    WORKER_SESSION_CACHE_METRICS
        .state_checkpoint_max_bytes
        .store(bytes, Ordering::Relaxed);
}

pub fn worker_session_cache_metrics_snapshot() -> WorkerSessionCacheMetricsSnapshot {
    let limits = worker_session_cache_limits();
    let active_sessions_current = WORKER_SESSION_INDEX.lock().unwrap().len();

    WorkerSessionCacheMetricsSnapshot {
        decisions_total: WORKER_SESSION_CACHE_METRICS
            .decisions_total
            .load(Ordering::Relaxed),
        session_task_total: WORKER_SESSION_CACHE_METRICS
            .session_task_total
            .load(Ordering::Relaxed),
        cold_total: WORKER_SESSION_CACHE_METRICS
            .cold_total
            .load(Ordering::Relaxed),
        bypass_total: WORKER_SESSION_CACHE_METRICS
            .bypass_total
            .load(Ordering::Relaxed),
        reset_total: WORKER_SESSION_CACHE_METRICS
            .reset_total
            .load(Ordering::Relaxed),
        unsupported_policy_total: WORKER_SESSION_CACHE_METRICS
            .unsupported_policy_total
            .load(Ordering::Relaxed),
        disabled_total: WORKER_SESSION_CACHE_METRICS
            .disabled_total
            .load(Ordering::Relaxed),
        kv_hit_total: WORKER_SESSION_CACHE_METRICS
            .kv_hit_total
            .load(Ordering::Relaxed),
        state_checkpoint_hit_total: WORKER_SESSION_CACHE_METRICS
            .state_checkpoint_hit_total
            .load(Ordering::Relaxed),
        state_checkpoint_miss_total: WORKER_SESSION_CACHE_METRICS
            .state_checkpoint_miss_total
            .load(Ordering::Relaxed),
        state_checkpoint_save_total: WORKER_SESSION_CACHE_METRICS
            .state_checkpoint_save_total
            .load(Ordering::Relaxed),
        state_checkpoint_reset_total: WORKER_SESSION_CACHE_METRICS
            .state_checkpoint_reset_total
            .load(Ordering::Relaxed),
        state_checkpoint_error_total: WORKER_SESSION_CACHE_METRICS
            .state_checkpoint_error_total
            .load(Ordering::Relaxed),
        state_checkpoint_quota_eviction_total: WORKER_SESSION_CACHE_METRICS
            .state_checkpoint_quota_eviction_total
            .load(Ordering::Relaxed),
        state_checkpoint_bytes_current: WORKER_SESSION_CACHE_METRICS
            .state_checkpoint_bytes_current
            .load(Ordering::Relaxed),
        state_checkpoint_max_bytes: WORKER_SESSION_CACHE_METRICS
            .state_checkpoint_max_bytes
            .load(Ordering::Relaxed),
        active_sessions_current,
        max_sessions: limits.max_entries,
        session_ttl_secs: limits.ttl.as_secs(),
        metadata_eviction_total: WORKER_SESSION_CACHE_METRICS
            .metadata_eviction_total
            .load(Ordering::Relaxed),
        metadata_stale_total: WORKER_SESSION_CACHE_METRICS
            .metadata_stale_total
            .load(Ordering::Relaxed),
    }
}

#[cfg(test)]
pub fn worker_session_cache_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
    LOCK.lock().unwrap()
}

#[cfg(test)]
pub fn set_worker_session_cache_limits_for_tests(max_entries: usize, ttl_secs: u64) {
    let mut limits = WORKER_SESSION_CACHE_LIMITS_FOR_TESTS.lock().unwrap();
    *limits = Some(WorkerSessionCacheLimits {
        max_entries: max_entries.max(1),
        ttl: Duration::from_secs(ttl_secs.max(1)),
    });
}

#[cfg(test)]
pub fn reset_worker_session_cache_metrics_for_tests() {
    WORKER_SESSION_CACHE_METRICS
        .decisions_total
        .store(0, Ordering::Relaxed);
    WORKER_SESSION_CACHE_METRICS
        .session_task_total
        .store(0, Ordering::Relaxed);
    WORKER_SESSION_CACHE_METRICS
        .cold_total
        .store(0, Ordering::Relaxed);
    WORKER_SESSION_CACHE_METRICS
        .bypass_total
        .store(0, Ordering::Relaxed);
    WORKER_SESSION_CACHE_METRICS
        .reset_total
        .store(0, Ordering::Relaxed);
    WORKER_SESSION_CACHE_METRICS
        .unsupported_policy_total
        .store(0, Ordering::Relaxed);
    WORKER_SESSION_CACHE_METRICS
        .disabled_total
        .store(0, Ordering::Relaxed);
    WORKER_SESSION_CACHE_METRICS
        .kv_hit_total
        .store(0, Ordering::Relaxed);
    WORKER_SESSION_CACHE_METRICS
        .state_checkpoint_hit_total
        .store(0, Ordering::Relaxed);
    WORKER_SESSION_CACHE_METRICS
        .state_checkpoint_miss_total
        .store(0, Ordering::Relaxed);
    WORKER_SESSION_CACHE_METRICS
        .state_checkpoint_save_total
        .store(0, Ordering::Relaxed);
    WORKER_SESSION_CACHE_METRICS
        .state_checkpoint_reset_total
        .store(0, Ordering::Relaxed);
    WORKER_SESSION_CACHE_METRICS
        .state_checkpoint_error_total
        .store(0, Ordering::Relaxed);
    WORKER_SESSION_CACHE_METRICS
        .state_checkpoint_quota_eviction_total
        .store(0, Ordering::Relaxed);
    WORKER_SESSION_CACHE_METRICS
        .state_checkpoint_bytes_current
        .store(0, Ordering::Relaxed);
    WORKER_SESSION_CACHE_METRICS
        .state_checkpoint_max_bytes
        .store(0, Ordering::Relaxed);
    WORKER_SESSION_CACHE_METRICS
        .metadata_eviction_total
        .store(0, Ordering::Relaxed);
    WORKER_SESSION_CACHE_METRICS
        .metadata_stale_total
        .store(0, Ordering::Relaxed);
    WORKER_SESSION_INDEX.lock().unwrap().clear();
    *WORKER_SESSION_CACHE_LIMITS_FOR_TESTS.lock().unwrap() = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cache_policy_case_insensitively() {
        assert_eq!(WorkerCachePolicy::parse(None), WorkerCachePolicy::Auto);
        assert_eq!(
            WorkerCachePolicy::parse(Some(" BYPASS ")),
            WorkerCachePolicy::Bypass
        );
        assert_eq!(
            WorkerCachePolicy::parse(Some("reset")),
            WorkerCachePolicy::Reset
        );
        assert_eq!(
            WorkerCachePolicy::parse(Some("something-else")),
            WorkerCachePolicy::Unknown
        );
    }

    #[test]
    fn records_cold_decision_without_claiming_kv_hit() {
        let _guard = worker_session_cache_test_guard();
        reset_worker_session_cache_metrics_for_tests();

        let decision = record_worker_cache_decision(Some("session-1234567890"), Some("auto"));
        assert_eq!(decision.status, WorkerCacheDecisionStatus::Cold);
        assert!(!decision.kv_reuse_enabled);
        assert_eq!(decision.session_hash.as_deref().map(str::len), Some(12));

        let metrics = worker_session_cache_metrics_snapshot();
        assert_eq!(metrics.decisions_total, 1);
        assert_eq!(metrics.session_task_total, 1);
        assert_eq!(metrics.cold_total, 1);
        assert_eq!(metrics.kv_hit_total, 0);
        assert_eq!(metrics.active_sessions_current, 1);
    }

    #[test]
    fn records_state_checkpoint_metrics_separately_from_kv_hits() {
        let _guard = worker_session_cache_test_guard();
        reset_worker_session_cache_metrics_for_tests();

        record_worker_state_checkpoint_hit();
        record_worker_state_checkpoint_miss();
        record_worker_state_checkpoint_save();
        record_worker_state_checkpoint_reset();
        record_worker_state_checkpoint_error();

        let metrics = worker_session_cache_metrics_snapshot();
        assert_eq!(metrics.kv_hit_total, 0);
        assert_eq!(metrics.state_checkpoint_hit_total, 1);
        assert_eq!(metrics.state_checkpoint_miss_total, 1);
        assert_eq!(metrics.state_checkpoint_save_total, 1);
        assert_eq!(metrics.state_checkpoint_reset_total, 1);
        assert_eq!(metrics.state_checkpoint_error_total, 1);
        assert_eq!(metrics.state_checkpoint_quota_eviction_total, 0);
        assert_eq!(metrics.state_checkpoint_bytes_current, 0);
        assert_eq!(metrics.state_checkpoint_max_bytes, 0);
    }

    #[test]
    fn records_state_checkpoint_quota_and_bytes_metrics() {
        let _guard = worker_session_cache_test_guard();
        reset_worker_session_cache_metrics_for_tests();

        record_worker_state_checkpoint_quota_eviction(2);
        set_worker_state_checkpoint_bytes_current(128);
        set_worker_state_checkpoint_max_bytes(1024);

        let metrics = worker_session_cache_metrics_snapshot();
        assert_eq!(metrics.state_checkpoint_quota_eviction_total, 2);
        assert_eq!(metrics.state_checkpoint_bytes_current, 128);
        assert_eq!(metrics.state_checkpoint_max_bytes, 1024);
    }

    #[test]
    fn records_bypass_reset_unknown_and_no_session() {
        let _guard = worker_session_cache_test_guard();
        reset_worker_session_cache_metrics_for_tests();

        let bypass = record_worker_cache_decision(Some("session-1234567890"), Some("bypass"));
        let reset = record_worker_cache_decision(Some("session-2234567890"), Some("reset"));
        let unknown = record_worker_cache_decision(Some("session-3234567890"), Some("pin"));
        let disabled = record_worker_cache_decision(None, None);

        assert_eq!(bypass.status, WorkerCacheDecisionStatus::Bypass);
        assert_eq!(reset.status, WorkerCacheDecisionStatus::Reset);
        assert_eq!(unknown.status, WorkerCacheDecisionStatus::UnsupportedPolicy);
        assert_eq!(disabled.status, WorkerCacheDecisionStatus::Disabled);

        let metrics = worker_session_cache_metrics_snapshot();
        assert_eq!(metrics.decisions_total, 4);
        assert_eq!(metrics.session_task_total, 3);
        assert_eq!(metrics.bypass_total, 1);
        assert_eq!(metrics.reset_total, 1);
        assert_eq!(metrics.unsupported_policy_total, 1);
        assert_eq!(metrics.disabled_total, 1);
        assert_eq!(metrics.kv_hit_total, 0);
        assert_eq!(metrics.active_sessions_current, 0);
    }

    #[test]
    fn tracks_session_index_reset_and_lru_eviction() {
        let _guard = worker_session_cache_test_guard();
        reset_worker_session_cache_metrics_for_tests();
        set_worker_session_cache_limits_for_tests(2, DEFAULT_WORKER_SESSION_TTL_SECS);

        record_worker_cache_decision(Some("session-1234567890"), Some("auto"));
        record_worker_cache_decision(Some("session-2234567890"), Some("auto"));
        record_worker_cache_decision(Some("session-3234567890"), Some("auto"));

        let metrics = worker_session_cache_metrics_snapshot();
        assert_eq!(metrics.active_sessions_current, 2);
        assert_eq!(metrics.metadata_eviction_total, 1);
        assert_eq!(metrics.max_sessions, 2);

        record_worker_cache_decision(Some("session-3234567890"), Some("reset"));
        let metrics = worker_session_cache_metrics_snapshot();
        assert_eq!(metrics.active_sessions_current, 1);
        assert_eq!(metrics.reset_total, 1);
    }

    #[test]
    fn evicts_stale_sessions_before_binding_new_one() {
        let _guard = worker_session_cache_test_guard();
        reset_worker_session_cache_metrics_for_tests();
        set_worker_session_cache_limits_for_tests(2, 1);

        {
            let mut sessions = WORKER_SESSION_INDEX.lock().unwrap();
            sessions.insert(
                "stale-session".to_string(),
                WorkerSessionEntry {
                    created_at: Instant::now() - Duration::from_secs(5),
                    last_used: Instant::now() - Duration::from_secs(5),
                },
            );
        }

        record_worker_cache_decision(Some("session-1234567890"), Some("auto"));

        let metrics = worker_session_cache_metrics_snapshot();
        assert_eq!(metrics.active_sessions_current, 1);
        assert_eq!(metrics.metadata_stale_total, 1);
    }
}
