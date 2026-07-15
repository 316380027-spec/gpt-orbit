use super::{BackendError, BackendResult, RpcNotification, RpcTransport};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum QuotaWindowKind {
    FiveHour,
    Weekly,
    Other,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    pub kind: QuotaWindowKind,
    pub used_percent: f64,
    pub remaining_percent: f64,
    pub window_duration_mins: f64,
    pub resets_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RateLimitSource {
    Read,
    Updated,
    Cache,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitState {
    pub five_hour: Option<QuotaWindow>,
    pub weekly: Option<QuotaWindow>,
    pub other: Vec<QuotaWindow>,
    pub plan_type: Option<String>,
    pub reached_type: Option<String>,
    pub fetched_at: i64,
    pub source: RateLimitSource,
    pub stale: bool,
}

pub fn merge_sparse(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(base), Value::Object(patch)) => {
            for (key, patch_value) in patch {
                match base.get_mut(key) {
                    Some(base_value) => merge_sparse(base_value, patch_value),
                    None => {
                        base.insert(key.clone(), patch_value.clone());
                    }
                }
            }
        }
        (base, patch) => *base = patch.clone(),
    }
}

pub fn normalize_rate_limits(
    raw: &Value,
    plan_type: Option<String>,
    fetched_at: i64,
    source: RateLimitSource,
) -> BackendResult<RateLimitState> {
    let limits = raw
        .as_object()
        .ok_or_else(|| BackendError::InvalidMessage("rate_limits_snapshot".to_string()))?;
    let mut candidates = Vec::new();

    if let Some(value) = limits.get("primary") {
        push_candidate(&mut candidates, value, Some(QuotaWindowKind::FiveHour))?;
    }
    if let Some(value) = limits.get("secondary") {
        push_candidate(&mut candidates, value, Some(QuotaWindowKind::Weekly))?;
    }
    let five_hour = take_preferred(&mut candidates, QuotaWindowKind::FiveHour);
    let weekly = take_preferred(&mut candidates, QuotaWindowKind::Weekly);
    let other = candidates
        .into_iter()
        .filter(|candidate| candidate.window.kind == QuotaWindowKind::Other)
        .map(|candidate| candidate.window)
        .collect();

    Ok(RateLimitState {
        five_hour,
        weekly,
        other,
        plan_type: plan_type.or_else(|| {
            limits
                .get("planType")
                .and_then(Value::as_str)
                .map(str::to_owned)
        }),
        reached_type: limits
            .get("rateLimitReachedType")
            .and_then(Value::as_str)
            .map(str::to_owned),
        fetched_at,
        source,
        stale: source == RateLimitSource::Cache,
    })
}

#[derive(Clone)]
struct Candidate {
    window: QuotaWindow,
}

fn push_candidate(
    candidates: &mut Vec<Candidate>,
    raw: &Value,
    fallback_kind: Option<QuotaWindowKind>,
) -> BackendResult<()> {
    if raw.is_null() {
        return Ok(());
    }
    let Some(raw) = raw.as_object() else {
        return Err(BackendError::InvalidMessage(
            "rate_limit_window".to_string(),
        ));
    };
    let used_percent = raw
        .get("usedPercent")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .ok_or_else(|| BackendError::InvalidMessage("rate_limit_usage".to_string()))?;
    let Some(window_duration_mins) = raw
        .get("windowDurationMins")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value > 0.0)
    else {
        return Ok(());
    };
    let kind = if (240.0..=360.0).contains(&window_duration_mins) {
        QuotaWindowKind::FiveHour
    } else if (9_360.0..=10_800.0).contains(&window_duration_mins) {
        QuotaWindowKind::Weekly
    } else {
        fallback_kind.unwrap_or(QuotaWindowKind::Other)
    };
    let used_percent = used_percent.clamp(0.0, 100.0);
    candidates.push(Candidate {
        window: QuotaWindow {
            kind,
            used_percent,
            remaining_percent: 100.0 - used_percent,
            window_duration_mins,
            resets_at: raw.get("resetsAt").and_then(valid_reset),
        },
    });
    Ok(())
}

fn valid_reset(value: &Value) -> Option<i64> {
    if let Some(value) = value.as_i64() {
        return (value > 0).then_some(value);
    }
    value
        .as_u64()
        .and_then(|value| i64::try_from(value).ok())
        .filter(|value| *value > 0)
}

fn take_preferred(candidates: &mut Vec<Candidate>, kind: QuotaWindowKind) -> Option<QuotaWindow> {
    let preferred = candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.window.kind == kind)
        .max_by_key(|(_, candidate)| candidate.window.resets_at.is_some())
        .map(|(index, _)| index)?;
    Some(candidates.remove(preferred).window)
}

struct RepositoryState {
    raw: Option<Value>,
    current: Option<RateLimitState>,
    revision: u64,
}

pub struct RateLimitRepository<T: RpcTransport> {
    rpc: T,
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
    state: Arc<Mutex<RepositoryState>>,
}

impl<T: RpcTransport> RateLimitRepository<T> {
    pub fn new(rpc: T, now: Arc<dyn Fn() -> i64 + Send + Sync>) -> Self {
        Self {
            rpc,
            now,
            state: Arc::new(Mutex::new(RepositoryState {
                raw: None,
                current: None,
                revision: 0,
            })),
        }
    }

    pub async fn refresh(&self) -> BackendResult<RateLimitState> {
        let started_at_revision = self.state.lock().unwrap().revision;
        let response = self
            .rpc
            .request_value("account/rateLimits/read", None)
            .await?;
        let raw = response
            .get("rateLimits")
            .cloned()
            .ok_or_else(|| BackendError::InvalidMessage("rate_limits_read_response".to_string()))?;
        let current = normalize_rate_limits(&raw, None, (self.now)(), RateLimitSource::Read)?;
        let mut state = self.state.lock().unwrap();
        if state.revision != started_at_revision {
            return state.current.clone().ok_or_else(|| {
                BackendError::InvalidMessage("rate_limits_repository_state".to_string())
            });
        }
        state.raw = Some(raw);
        state.current = Some(current.clone());
        state.revision += 1;
        Ok(current)
    }

    pub async fn apply_notification(
        &self,
        note: &RpcNotification,
    ) -> BackendResult<Option<RateLimitState>> {
        if note.method != "account/rateLimits/updated" {
            return Ok(None);
        }
        let patch = note.params.get("rateLimits").ok_or_else(|| {
            BackendError::InvalidMessage("rate_limits_update_notification".to_string())
        })?;
        if self.state.lock().unwrap().raw.is_none() {
            self.refresh().await?;
        }
        let fetched_at = (self.now)();
        let mut state = self.state.lock().unwrap();
        let mut raw = state.raw.clone().ok_or_else(|| {
            BackendError::InvalidMessage("rate_limits_repository_state".to_string())
        })?;
        merge_sparse(&mut raw, patch);
        let current = normalize_rate_limits(&raw, None, fetched_at, RateLimitSource::Updated)?;
        state.raw = Some(raw);
        state.current = Some(current.clone());
        state.revision += 1;
        Ok(Some(current))
    }

    pub async fn current(&self) -> Option<RateLimitState> {
        self.state.lock().unwrap().current.clone()
    }

    pub fn reset(&self) {
        let mut state = self.state.lock().unwrap();
        state.raw = None;
        state.current = None;
        state.revision = state.revision.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        merge_sparse, normalize_rate_limits, QuotaWindowKind, RateLimitRepository, RateLimitSource,
    };
    use crate::backend::{BackendError, BackendResult, RpcNotification, RpcTransport};
    use serde_json::{json, Value};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier, Mutex};
    use tokio::sync::Semaphore;

    #[test]
    fn sparse_patch_keeps_missing_weekly() {
        let mut base = json!({
            "primary": {"usedPercent": 10, "windowDurationMins": 300},
            "secondary": {"usedPercent": 20, "windowDurationMins": 10080}
        });

        merge_sparse(&mut base, &json!({"primary": {"usedPercent": 35}}));

        assert_eq!(base["primary"]["usedPercent"], 35);
        assert_eq!(base["primary"]["windowDurationMins"], 300);
        assert_eq!(base["secondary"]["usedPercent"], 20);
    }

    #[test]
    fn explicit_null_clears_weekly() {
        let mut base = json!({
            "primary": {"usedPercent": 10, "windowDurationMins": 300},
            "secondary": {"usedPercent": 20, "windowDurationMins": 10080}
        });

        merge_sparse(&mut base, &json!({"secondary": null}));

        assert!(base["secondary"].is_null());
    }

    #[test]
    fn classifies_300_as_five_hour() {
        let state = normalize_rate_limits(
            &json!({
                "primary": {"usedPercent": 25, "windowDurationMins": 300, "resetsAt": 1_800_000_000}
            }),
            Some("plus".to_string()),
            1_700_000_000,
            RateLimitSource::Read,
        )
        .unwrap();

        assert_eq!(state.five_hour.unwrap().kind, QuotaWindowKind::FiveHour);
        assert!(state.weekly.is_none());
    }

    #[test]
    fn classifies_10080_as_weekly() {
        let state = normalize_rate_limits(
            &json!({
                "secondary": {"usedPercent": 40, "windowDurationMins": 10080, "resetsAt": 1_800_500_000}
            }),
            None,
            1_700_000_000,
            RateLimitSource::Read,
        )
        .unwrap();

        assert_eq!(state.weekly.unwrap().kind, QuotaWindowKind::Weekly);
        assert!(state.five_hour.is_none());
    }

    #[test]
    fn clamps_used_percent() {
        let state = normalize_rate_limits(
            &json!({
                "primary": {"usedPercent": 125, "windowDurationMins": 300, "resetsAt": "invalid"}
            }),
            None,
            1_700_000_000,
            RateLimitSource::Updated,
        )
        .unwrap();
        let window = state.five_hour.unwrap();

        assert_eq!(window.used_percent, 100.0);
        assert_eq!(window.remaining_percent, 0.0);
        assert_eq!(window.resets_at, None);
    }

    #[test]
    fn rejects_non_numeric_usage() {
        let error = normalize_rate_limits(
            &json!({
                "primary": {"usedPercent": "all", "windowDurationMins": 300}
            }),
            None,
            1_700_000_000,
            RateLimitSource::Read,
        )
        .unwrap_err();

        assert!(matches!(
            error,
            BackendError::InvalidMessage(category) if category == "rate_limit_usage"
        ));
    }

    type RecordedCalls = Arc<Mutex<Vec<(&'static str, Option<Value>)>>>;

    #[derive(Clone, Default)]
    struct MockTransport {
        calls: RecordedCalls,
    }

    #[async_trait::async_trait]
    impl RpcTransport for MockTransport {
        async fn request_value(
            &self,
            method: &'static str,
            params: Option<Value>,
        ) -> BackendResult<Value> {
            self.calls.lock().unwrap().push((method, params));
            Ok(json!({
                "rateLimits": {
                    "primary": {"usedPercent": 30, "windowDurationMins": 300},
                    "secondary": {"usedPercent": 50, "windowDurationMins": 10080},
                    "planType": "plus"
                },
                "ignoredUnknownField": {"future": true}
            }))
        }
    }

    #[tokio::test]
    async fn notification_without_read_baseline_refetches() {
        let rpc = MockTransport::default();
        let repository = RateLimitRepository::new(rpc.clone(), Arc::new(|| 1_700_000_000));

        let state = repository
            .apply_notification(&RpcNotification {
                method: "account/rateLimits/updated".to_string(),
                params: json!({
                    "rateLimits": {"primary": {"usedPercent": 55}}
                }),
            })
            .await
            .unwrap()
            .unwrap();

        assert_eq!(
            rpc.calls.lock().unwrap().as_slice(),
            &[("account/rateLimits/read", None)]
        );
        assert_eq!(state.source, RateLimitSource::Updated);
        assert_eq!(state.plan_type.as_deref(), Some("plus"));
        assert_eq!(state.five_hour.unwrap().used_percent, 55.0);
        assert_eq!(state.weekly.unwrap().used_percent, 50.0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_notifications_merge_against_latest_baseline() {
        let rpc = MockTransport::default();
        let barrier = Arc::new(Barrier::new(2));
        let now_calls = Arc::new(AtomicUsize::new(0));
        let now = {
            let barrier = Arc::clone(&barrier);
            let now_calls = Arc::clone(&now_calls);
            Arc::new(move || {
                if now_calls.fetch_add(1, Ordering::SeqCst) > 0 {
                    barrier.wait();
                }
                1_700_000_000
            })
        };
        let repository = Arc::new(RateLimitRepository::new(rpc, now));
        repository.refresh().await.unwrap();

        let primary_repository = Arc::clone(&repository);
        let primary = tokio::spawn(async move {
            primary_repository
                .apply_notification(&RpcNotification {
                    method: "account/rateLimits/updated".to_string(),
                    params: json!({"rateLimits": {"primary": {"usedPercent": 11}}}),
                })
                .await
        });
        let secondary_repository = Arc::clone(&repository);
        let secondary = tokio::spawn(async move {
            secondary_repository
                .apply_notification(&RpcNotification {
                    method: "account/rateLimits/updated".to_string(),
                    params: json!({"rateLimits": {"secondary": {"usedPercent": 22}}}),
                })
                .await
        });

        primary.await.unwrap().unwrap();
        secondary.await.unwrap().unwrap();
        let current = repository.current().await.unwrap();
        assert_eq!(current.five_hour.unwrap().used_percent, 11.0);
        assert_eq!(current.weekly.unwrap().used_percent, 22.0);
    }

    #[derive(Clone)]
    struct RefreshRaceTransport {
        calls: Arc<AtomicUsize>,
        refresh_started: Arc<Semaphore>,
        release_refresh: Arc<Semaphore>,
    }

    #[async_trait::async_trait]
    impl RpcTransport for RefreshRaceTransport {
        async fn request_value(
            &self,
            method: &'static str,
            params: Option<Value>,
        ) -> BackendResult<Value> {
            assert_eq!(method, "account/rateLimits/read");
            assert_eq!(params, None);
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            if call > 0 {
                self.refresh_started.add_permits(1);
                self.release_refresh.acquire().await.unwrap().forget();
            }
            Ok(json!({
                "rateLimits": {
                    "primary": {
                        "usedPercent": if call == 0 { 10 } else { 20 },
                        "windowDurationMins": 300
                    },
                    "secondary": {"usedPercent": 30, "windowDurationMins": 10080},
                    "planType": "plus"
                }
            }))
        }
    }

    #[tokio::test]
    async fn stale_refresh_does_not_overwrite_notification() {
        let rpc = RefreshRaceTransport {
            calls: Arc::new(AtomicUsize::new(0)),
            refresh_started: Arc::new(Semaphore::new(0)),
            release_refresh: Arc::new(Semaphore::new(0)),
        };
        let repository = Arc::new(RateLimitRepository::new(
            rpc.clone(),
            Arc::new(|| 1_700_000_000),
        ));
        repository.refresh().await.unwrap();

        let refresh_repository = Arc::clone(&repository);
        let refresh = tokio::spawn(async move { refresh_repository.refresh().await });
        rpc.refresh_started.acquire().await.unwrap().forget();
        repository
            .apply_notification(&RpcNotification {
                method: "account/rateLimits/updated".to_string(),
                params: json!({"rateLimits": {"primary": {"usedPercent": 60}}}),
            })
            .await
            .unwrap();
        rpc.release_refresh.add_permits(1);

        let returned = refresh.await.unwrap().unwrap();
        let current = repository.current().await.unwrap();
        assert_eq!(returned, current);
        assert_eq!(current.source, RateLimitSource::Updated);
        assert_eq!(current.five_hour.unwrap().used_percent, 60.0);
    }

    #[tokio::test]
    async fn notification_plan_type_patch_replaces_baseline() {
        let repository =
            RateLimitRepository::new(MockTransport::default(), Arc::new(|| 1_700_000_000));
        repository.refresh().await.unwrap();

        let state = repository
            .apply_notification(&RpcNotification {
                method: "account/rateLimits/updated".to_string(),
                params: json!({"rateLimits": {"planType": "pro"}}),
            })
            .await
            .unwrap()
            .unwrap();

        assert_eq!(state.plan_type.as_deref(), Some("pro"));

        let cleared = repository
            .apply_notification(&RpcNotification {
                method: "account/rateLimits/updated".to_string(),
                params: json!({"rateLimits": {"planType": null}}),
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(cleared.plan_type, None);
    }

    #[tokio::test]
    async fn normalization_failure_leaves_repository_state_unchanged() {
        let repository =
            RateLimitRepository::new(MockTransport::default(), Arc::new(|| 1_700_000_000));
        let baseline = repository.refresh().await.unwrap();

        let result = repository
            .apply_notification(&RpcNotification {
                method: "account/rateLimits/updated".to_string(),
                params: json!({"rateLimits": {"primary": {"usedPercent": "secret-invalid"}}}),
            })
            .await;

        assert!(matches!(result, Err(BackendError::InvalidMessage(_))));
        assert_eq!(repository.current().await, Some(baseline));
    }

    #[tokio::test]
    async fn reset_discards_raw_and_current_before_account_change() {
        let repository =
            RateLimitRepository::new(MockTransport::default(), Arc::new(|| 1_700_000_000));
        repository.refresh().await.unwrap();

        repository.reset();

        assert_eq!(repository.current().await, None);
        let state = repository
            .apply_notification(&RpcNotification {
                method: "account/rateLimits/updated".to_string(),
                params: json!({"rateLimits": {"primary": {"usedPercent": 80}}}),
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(state.weekly.unwrap().used_percent, 50.0);
    }

    #[derive(Clone)]
    struct ConcurrentInitialReadTransport {
        reads_started: Arc<Barrier>,
    }

    #[async_trait::async_trait]
    impl RpcTransport for ConcurrentInitialReadTransport {
        async fn request_value(
            &self,
            method: &'static str,
            params: Option<Value>,
        ) -> BackendResult<Value> {
            assert_eq!(method, "account/rateLimits/read");
            assert_eq!(params, None);
            self.reads_started.wait();
            Ok(json!({
                "rateLimits": {
                    "primary": {"usedPercent": 10, "windowDurationMins": 300},
                    "secondary": {"usedPercent": 20, "windowDurationMins": 10080}
                }
            }))
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_notifications_without_baseline_keep_both_patches() {
        let repository = Arc::new(RateLimitRepository::new(
            ConcurrentInitialReadTransport {
                reads_started: Arc::new(Barrier::new(2)),
            },
            Arc::new(|| 1_700_000_000),
        ));
        let primary_repository = Arc::clone(&repository);
        let primary = tokio::spawn(async move {
            primary_repository
                .apply_notification(&RpcNotification {
                    method: "account/rateLimits/updated".to_string(),
                    params: json!({"rateLimits": {"primary": {"usedPercent": 45}}}),
                })
                .await
        });
        let secondary_repository = Arc::clone(&repository);
        let secondary = tokio::spawn(async move {
            secondary_repository
                .apply_notification(&RpcNotification {
                    method: "account/rateLimits/updated".to_string(),
                    params: json!({"rateLimits": {"secondary": {"usedPercent": 65}}}),
                })
                .await
        });

        primary.await.unwrap().unwrap();
        secondary.await.unwrap().unwrap();
        let current = repository.current().await.unwrap();
        assert_eq!(current.five_hour.unwrap().used_percent, 45.0);
        assert_eq!(current.weekly.unwrap().used_percent, 65.0);
    }

    #[test]
    fn duration_classification_includes_boundaries_only() {
        let cases = [
            ("secondary", 240, QuotaWindowKind::FiveHour),
            ("secondary", 360, QuotaWindowKind::FiveHour),
            ("secondary", 239, QuotaWindowKind::Weekly),
            ("secondary", 361, QuotaWindowKind::Weekly),
            ("primary", 9_360, QuotaWindowKind::Weekly),
            ("primary", 10_800, QuotaWindowKind::Weekly),
            ("primary", 9_359, QuotaWindowKind::FiveHour),
            ("primary", 10_801, QuotaWindowKind::FiveHour),
        ];

        for (slot, duration, expected) in cases {
            let state = normalize_rate_limits(
                &json!({slot: {"usedPercent": 5, "windowDurationMins": duration}}),
                None,
                1_700_000_000,
                RateLimitSource::Read,
            )
            .unwrap();
            let actual = state.five_hour.or(state.weekly).unwrap().kind;
            assert_eq!(actual, expected, "slot={slot}, duration={duration}");
        }
    }

    #[test]
    fn clamps_negative_usage_and_discards_invalid_resets() {
        for reset in [json!(-1), json!("later"), json!(u64::MAX)] {
            let state = normalize_rate_limits(
                &json!({
                    "primary": {
                        "usedPercent": -25,
                        "windowDurationMins": 300,
                        "resetsAt": reset
                    }
                }),
                None,
                1_700_000_000,
                RateLimitSource::Read,
            )
            .unwrap();
            let window = state.five_hour.unwrap();
            assert_eq!(window.used_percent, 0.0);
            assert_eq!(window.remaining_percent, 100.0);
            assert_eq!(window.resets_at, None);
        }
    }

    #[test]
    fn serializes_frontend_contract_as_camel_case() {
        let state = normalize_rate_limits(
            &json!({
                "primary": {"usedPercent": 25, "windowDurationMins": 300}
            }),
            Some("plus".to_string()),
            1_700_000_000,
            RateLimitSource::Read,
        )
        .unwrap();

        let serialized = serde_json::to_value(state).unwrap();
        assert!(serialized.get("fiveHour").is_some());
        assert!(serialized.get("planType").is_some());
        assert!(serialized.get("fetchedAt").is_some());
        assert!(serialized["fiveHour"].get("usedPercent").is_some());
        assert!(serialized["fiveHour"].get("remainingPercent").is_some());
        assert!(serialized["fiveHour"].get("windowDurationMins").is_some());
        assert!(serialized["fiveHour"].get("resetsAt").is_some());
    }
}
