use super::{BackendError, BackendResult, RateLimitSource, RateLimitState};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub struct RateLimitCache {
    path: PathBuf,
}

impl RateLimitCache {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub async fn load(&self) -> BackendResult<Option<RateLimitState>> {
        let bytes = match tokio::fs::read(&self.path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(BackendError::RpcError("cache_read".to_string())),
        };
        let mut state = match serde_json::from_slice::<RateLimitState>(&bytes) {
            Ok(state) => state,
            Err(_) => {
                tracing::warn!(category = "cache_invalid");
                return Ok(None);
            }
        };
        state.source = RateLimitSource::Cache;
        state.stale = true;
        Ok(Some(state))
    }

    pub async fn store(&self, state: &RateLimitState) -> BackendResult<()> {
        let bytes = serde_json::to_vec(state)
            .map_err(|_| BackendError::RpcError("cache_encode".to_string()))?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| BackendError::RpcError("cache_path".to_string()))?;
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|_| BackendError::RpcError("cache_write".to_string()))?;
        let extension = format!(
            "tmp-{}-{}",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        );
        let temporary = self.path.with_extension(extension);
        if tokio::fs::write(&temporary, bytes).await.is_err() {
            return Err(BackendError::RpcError("cache_write".to_string()));
        }
        if tokio::fs::rename(&temporary, &self.path).await.is_err() {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(BackendError::RpcError("cache_write".to_string()));
        }
        Ok(())
    }

    pub async fn clear(&self) -> BackendResult<()> {
        match tokio::fs::remove_file(&self.path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(BackendError::RpcError("cache_clear".to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RateLimitCache;
    use crate::backend::{QuotaWindow, QuotaWindowKind, RateLimitSource, RateLimitState};
    use serde_json::Value;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    fn path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "codex-orbit-cache-test-{}-{}.json",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn state() -> RateLimitState {
        RateLimitState {
            five_hour: Some(QuotaWindow {
                kind: QuotaWindowKind::FiveHour,
                used_percent: 25.0,
                remaining_percent: 75.0,
                window_duration_mins: 300.0,
                resets_at: Some(1_800_000_000),
            }),
            weekly: None,
            other: Vec::new(),
            plan_type: Some("plus".to_string()),
            reached_type: None,
            fetched_at: 1_700_000_000,
            source: RateLimitSource::Read,
            stale: false,
        }
    }

    #[tokio::test]
    async fn restored_cache_is_always_stale() {
        let path = path();
        let cache = RateLimitCache::new(path.clone());
        cache.store(&state()).await.unwrap();

        let restored = cache.load().await.unwrap().unwrap();
        let _ = std::fs::remove_file(path);

        assert_eq!(restored.source, RateLimitSource::Cache);
        assert!(restored.stale);
    }

    #[tokio::test]
    async fn cache_has_no_sensitive_keys() {
        let path = path();
        let cache = RateLimitCache::new(path.clone());
        cache.store(&state()).await.unwrap();
        let serialized: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let _ = std::fs::remove_file(path);

        let object = serialized.as_object().unwrap();
        assert_eq!(
            object.keys().map(String::as_str).collect::<Vec<_>>(),
            vec![
                "fetchedAt",
                "fiveHour",
                "other",
                "planType",
                "reachedType",
                "source",
                "stale",
                "weekly"
            ]
        );
        let text = serialized.to_string().to_ascii_lowercase();
        for forbidden in ["token", "email", "authurl", "loginid", "raw", "response"] {
            assert!(!text.contains(forbidden), "cache contained {forbidden}");
        }
    }

    #[tokio::test]
    async fn corrupt_cache_is_treated_as_empty() {
        let path = path();
        std::fs::write(&path, b"{not-json").unwrap();
        let cache = RateLimitCache::new(path.clone());

        assert_eq!(cache.load().await.unwrap(), None);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn store_replaces_existing_target() {
        let path = path();
        let cache = RateLimitCache::new(path.clone());
        let mut first = state();
        first.fetched_at = 1;
        cache.store(&first).await.unwrap();
        let mut second = state();
        second.fetched_at = 2;

        cache.store(&second).await.unwrap();

        assert_eq!(cache.load().await.unwrap().unwrap().fetched_at, 2);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn clear_removes_cross_account_cache() {
        let path = path();
        let cache = RateLimitCache::new(path.clone());
        cache.store(&state()).await.unwrap();

        cache.clear().await.unwrap();

        assert_eq!(cache.load().await.unwrap(), None);
    }
}
