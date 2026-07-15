use super::{
    BackendError, BackendResult, RefreshReason, ResetCreditCache, ResetCreditState,
    ResetCreditTransport,
};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::Emitter;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;

const UPDATED_EVENT: &str = "reset-credits://updated";
pub(crate) const REFRESH_INTERVAL: Duration = Duration::from_secs(300);

pub enum ResetCreditCommand {
    Refresh(RefreshReason),
    Shutdown,
}

#[derive(Clone)]
pub struct ResetCreditService {
    current: Arc<StdMutex<Option<ResetCreditState>>>,
    commands: mpsc::UnboundedSender<ResetCreditCommand>,
    task: Arc<Mutex<Option<JoinHandle<()>>>>,
}

#[doc(hidden)]
pub trait ResetCreditServiceEvents: Clone + Send + Sync + 'static {
    fn state(&self, state: &ResetCreditState);
}

#[derive(Clone)]
struct AppEvents {
    app: tauri::AppHandle,
}

impl ResetCreditServiceEvents for AppEvents {
    fn state(&self, state: &ResetCreditState) {
        let _ = self.app.emit(UPDATED_EVENT, state);
    }
}

impl ResetCreditService {
    pub async fn start<T>(
        app: tauri::AppHandle,
        transport: T,
        cache: ResetCreditCache,
    ) -> BackendResult<Self>
    where
        T: ResetCreditTransport + 'static,
    {
        Self::start_with_events(transport, cache, AppEvents { app }).await
    }

    #[doc(hidden)]
    pub async fn start_with_events<T, E>(
        transport: T,
        cache: ResetCreditCache,
        events: E,
    ) -> BackendResult<Self>
    where
        T: ResetCreditTransport + 'static,
        E: ResetCreditServiceEvents,
    {
        let restored = cache.load().await?;
        let current = Arc::new(StdMutex::new(restored.clone()));
        if let Some(state) = &restored {
            events.state(state);
        }

        let (commands, command_rx) = mpsc::unbounded_channel();
        let task = Arc::new(Mutex::new(None));
        let worker_current = Arc::clone(&current);
        let handle = tokio::spawn(run_worker(
            transport,
            cache,
            events,
            worker_current,
            command_rx,
        ));
        *task.lock().await = Some(handle);

        Ok(Self {
            current,
            commands,
            task,
        })
    }

    pub fn current(&self) -> Option<ResetCreditState> {
        self.current.lock().unwrap().clone()
    }

    pub fn refresh_now(&self, reason: RefreshReason) -> BackendResult<()> {
        self.commands
            .send(ResetCreditCommand::Refresh(reason))
            .map_err(|_| BackendError::RpcDisconnected)
    }

    pub async fn shutdown(&self) -> BackendResult<()> {
        let _ = self.commands.send(ResetCreditCommand::Shutdown);
        let mut task = self.task.lock().await;
        if let Some(handle) = task.take() {
            handle
                .await
                .map_err(|_| BackendError::RpcError("reset_credit_worker".to_string()))?;
        }
        Ok(())
    }
}

async fn run_worker<T, E>(
    transport: T,
    cache: ResetCreditCache,
    events: E,
    current: Arc<StdMutex<Option<ResetCreditState>>>,
    mut commands: mpsc::UnboundedReceiver<ResetCreditCommand>,
) where
    T: ResetCreditTransport,
    E: ResetCreditServiceEvents,
{
    let mut interval = tokio::time::interval_at(
        tokio::time::Instant::now() + REFRESH_INTERVAL,
        REFRESH_INTERVAL,
    );
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut refresh_requested = false;

    loop {
        if !refresh_requested {
            tokio::select! {
                command = commands.recv() => match command {
                    Some(ResetCreditCommand::Refresh(reason)) => {
                        let _ = reason;
                        refresh_requested = true;
                    }
                    Some(ResetCreditCommand::Shutdown) | None => return,
                },
                _ = interval.tick() => refresh_requested = true,
            }
        }

        if drain_commands(&mut commands, &mut refresh_requested) {
            return;
        }
        if !refresh_requested {
            continue;
        }
        refresh_requested = false;

        let fetch = transport.fetch();
        tokio::pin!(fetch);
        let result = loop {
            tokio::select! {
                result = &mut fetch => break result,
                command = commands.recv() => match command {
                    Some(ResetCreditCommand::Refresh(reason)) => {
                        let _ = reason;
                        refresh_requested = true;
                    }
                    Some(ResetCreditCommand::Shutdown) | None => return,
                },
                _ = interval.tick() => refresh_requested = true,
            }
        };

        publish_result(&cache, &events, &current, result).await;
        if drain_commands(&mut commands, &mut refresh_requested) {
            return;
        }
    }
}

fn drain_commands(
    commands: &mut mpsc::UnboundedReceiver<ResetCreditCommand>,
    refresh_requested: &mut bool,
) -> bool {
    while let Ok(command) = commands.try_recv() {
        match command {
            ResetCreditCommand::Refresh(reason) => {
                let _ = reason;
                *refresh_requested = true;
            }
            ResetCreditCommand::Shutdown => return true,
        }
    }
    false
}

async fn publish_result<E: ResetCreditServiceEvents>(
    cache: &ResetCreditCache,
    events: &E,
    current: &Arc<StdMutex<Option<ResetCreditState>>>,
    result: BackendResult<ResetCreditState>,
) {
    match result {
        Ok(mut state) => {
            state.stale = false;
            state.auth_required = false;
            if cache.store(&state).await.is_err() {
                tracing::warn!(category = "reset_credit_cache_write_failed");
            }
            *current.lock().unwrap() = Some(state.clone());
            events.state(&state);
        }
        Err(error) => {
            let auth_required = matches!(error, BackendError::AuthenticationRequired);
            let stale = {
                let mut current = current.lock().unwrap();
                if current.is_none() && auth_required {
                    *current = Some(ResetCreditState {
                        available_count: None,
                        fetched_at: unix_timestamp_now(),
                        stale: true,
                        auth_required: true,
                    });
                }
                current.as_mut().map(|state| {
                    state.stale = true;
                    state.auth_required = auth_required;
                    state.clone()
                })
            };
            if let Some(state) = stale {
                if cache.store(&state).await.is_err() {
                    tracing::warn!(category = "reset_credit_cache_write_failed");
                }
                events.state(&state);
            }
        }
    }
}

fn unix_timestamp_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{ResetCreditService, ResetCreditServiceEvents, REFRESH_INTERVAL};
    use crate::backend::{
        BackendError, BackendResult, RefreshReason, ResetCreditCache, ResetCreditState,
        ResetCreditTransport,
    };
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::Semaphore;

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    fn temp_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "codex-orbit-reset-credit-service-{label}-{}-{}.json",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn state(available_count: u32, fetched_at: i64) -> ResetCreditState {
        ResetCreditState {
            available_count: Some(available_count),
            fetched_at,
            stale: false,
            auth_required: false,
        }
    }

    enum FetchStep {
        Ready(BackendResult<ResetCreditState>),
        Gated(Arc<Semaphore>, BackendResult<ResetCreditState>),
    }

    #[derive(Clone)]
    struct FakeTransport {
        steps: Arc<Mutex<VecDeque<FetchStep>>>,
        calls: Arc<AtomicUsize>,
    }

    impl FakeTransport {
        fn new(steps: impl IntoIterator<Item = FetchStep>) -> Self {
            Self {
                steps: Arc::new(Mutex::new(steps.into_iter().collect())),
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl ResetCreditTransport for FakeTransport {
        async fn fetch(&self) -> BackendResult<ResetCreditState> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let step = self.steps.lock().unwrap().pop_front().unwrap_or_else(|| {
                FetchStep::Ready(Err(BackendError::RpcError("fake_exhausted".to_string())))
            });
            match step {
                FetchStep::Ready(result) => result,
                FetchStep::Gated(gate, result) => {
                    gate.acquire().await.unwrap().forget();
                    result
                }
            }
        }
    }

    #[derive(Clone, Default)]
    struct RecordingEvents {
        states: Arc<Mutex<Vec<ResetCreditState>>>,
    }

    impl ResetCreditServiceEvents for RecordingEvents {
        fn state(&self, state: &ResetCreditState) {
            self.states.lock().unwrap().push(state.clone());
        }
    }

    async fn wait_for_calls(transport: &FakeTransport, expected: usize) {
        tokio::time::timeout(Duration::from_secs(1), async {
            while transport.calls() < expected {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("fake transport call timed out");
    }

    async fn wait_for_count(service: &ResetCreditService, expected: u32) -> ResetCreditState {
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Some(current) = service.current() {
                    if current.available_count == Some(expected) {
                        break current;
                    }
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("reset-credit state timed out")
    }

    #[tokio::test]
    async fn startup_waits_for_an_explicit_refresh_after_window_restore() {
        let path = temp_path("deferred-startup");
        let transport = FakeTransport::new([FetchStep::Ready(Ok(state(3, 20)))]);
        let service = ResetCreditService::start_with_events(
            transport.clone(),
            ResetCreditCache::new(path.clone()),
            RecordingEvents::default(),
        )
        .await
        .unwrap();

        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert_eq!(transport.calls(), 0);

        service.refresh_now(RefreshReason::Startup).unwrap();
        wait_for_count(&service, 3).await;
        assert_eq!(transport.calls(), 1);

        service.shutdown().await.unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn startup_loads_stale_cache_then_publishes_live_count() {
        let path = temp_path("startup");
        let cache = ResetCreditCache::new(path.clone());
        cache.store(&state(4, 10)).await.unwrap();
        let transport = FakeTransport::new([FetchStep::Ready(Ok(state(3, 20)))]);
        let events = RecordingEvents::default();

        let service =
            ResetCreditService::start_with_events(transport.clone(), cache, events.clone())
                .await
                .unwrap();

        service.refresh_now(RefreshReason::Startup).unwrap();

        let restored = service.current().unwrap();
        assert_eq!(restored.available_count, Some(4));
        assert!(restored.stale);
        let live = wait_for_count(&service, 3).await;
        assert!(!live.stale);
        assert_eq!(events.states.lock().unwrap()[0], restored);
        assert_eq!(transport.calls(), 1);

        service.shutdown().await.unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn failed_refresh_retains_previous_count_as_stale() {
        let path = temp_path("failure");
        let cache = ResetCreditCache::new(path.clone());
        let transport = FakeTransport::new([
            FetchStep::Ready(Ok(state(3, 20))),
            FetchStep::Ready(Err(BackendError::RpcError("synthetic".to_string()))),
        ]);
        let service = ResetCreditService::start_with_events(
            transport.clone(),
            cache,
            RecordingEvents::default(),
        )
        .await
        .unwrap();
        service.refresh_now(RefreshReason::Startup).unwrap();
        wait_for_count(&service, 3).await;

        service.refresh_now(RefreshReason::Manual).unwrap();
        wait_for_calls(&transport, 2).await;
        tokio::task::yield_now().await;

        let retained = service.current().unwrap();
        assert_eq!(retained.available_count, Some(3));
        assert!(retained.stale);

        service.shutdown().await.unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn authentication_required_preserves_cached_count_as_stale_typed_state() {
        let path = temp_path("auth-required");
        let cache = ResetCreditCache::new(path.clone());
        cache.store(&state(4, 10)).await.unwrap();
        let transport =
            FakeTransport::new([FetchStep::Ready(Err(BackendError::AuthenticationRequired))]);
        let events = RecordingEvents::default();
        let service =
            ResetCreditService::start_with_events(transport, cache.clone(), events.clone())
                .await
                .unwrap();

        service.refresh_now(RefreshReason::Startup).unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let current_is_auth_required =
                    service.current().is_some_and(|state| state.auth_required);
                let event_is_auth_required = events
                    .states
                    .lock()
                    .unwrap()
                    .last()
                    .is_some_and(|state| state.auth_required);
                if current_is_auth_required && event_is_auth_required {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("auth-required state timed out");

        let current = service.current().unwrap();
        assert_eq!(current.available_count, Some(4));
        assert!(current.stale);
        assert!(current.auth_required);
        assert_eq!(events.states.lock().unwrap().last(), Some(&current));
        assert!(cache.load().await.unwrap().unwrap().auth_required);

        service.shutdown().await.unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn authentication_required_without_cache_publishes_unknown_typed_state() {
        let path = temp_path("auth-required-no-cache");
        let cache = ResetCreditCache::new(path.clone());
        let transport =
            FakeTransport::new([FetchStep::Ready(Err(BackendError::AuthenticationRequired))]);
        let events = RecordingEvents::default();
        let service =
            ResetCreditService::start_with_events(transport, cache.clone(), events.clone())
                .await
                .unwrap();

        service.refresh_now(RefreshReason::Startup).unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if events
                    .states
                    .lock()
                    .unwrap()
                    .last()
                    .is_some_and(|state| state.auth_required)
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("auth-required event timed out");

        let current = service.current().unwrap();
        assert_eq!(current.available_count, None);
        assert!(current.stale);
        assert!(current.auth_required);
        assert_eq!(events.states.lock().unwrap().last(), Some(&current));
        assert!(cache.load().await.unwrap().unwrap().auth_required);

        service.shutdown().await.unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn concurrent_refreshes_coalesce_to_one_pending_fetch() {
        let path = temp_path("coalesced");
        let gate = Arc::new(Semaphore::new(0));
        let transport = FakeTransport::new([
            FetchStep::Ready(Ok(state(1, 10))),
            FetchStep::Gated(Arc::clone(&gate), Ok(state(2, 20))),
            FetchStep::Ready(Ok(state(3, 30))),
        ]);
        let service = ResetCreditService::start_with_events(
            transport.clone(),
            ResetCreditCache::new(path.clone()),
            RecordingEvents::default(),
        )
        .await
        .unwrap();
        service.refresh_now(RefreshReason::Startup).unwrap();
        wait_for_count(&service, 1).await;

        service.refresh_now(RefreshReason::Manual).unwrap();
        wait_for_calls(&transport, 2).await;
        for _ in 0..32 {
            service.refresh_now(RefreshReason::Tray).unwrap();
        }
        gate.add_permits(1);
        wait_for_calls(&transport, 3).await;
        wait_for_count(&service, 3).await;
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert_eq!(transport.calls(), 3);

        service.shutdown().await.unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test(start_paused = true)]
    async fn periodic_refresh_uses_the_300_second_production_period() {
        assert_eq!(REFRESH_INTERVAL, Duration::from_secs(300));
        let path = temp_path("periodic");
        let transport = FakeTransport::new([
            FetchStep::Ready(Ok(state(1, 10))),
            FetchStep::Ready(Ok(state(2, 20))),
        ]);
        let service = ResetCreditService::start_with_events(
            transport.clone(),
            ResetCreditCache::new(path.clone()),
            RecordingEvents::default(),
        )
        .await
        .unwrap();

        service.refresh_now(RefreshReason::Startup).unwrap();
        wait_for_calls(&transport, 1).await;
        tokio::time::advance(Duration::from_secs(299)).await;
        tokio::task::yield_now().await;
        assert_eq!(transport.calls(), 1);
        tokio::time::advance(Duration::from_secs(1)).await;
        wait_for_calls(&transport, 2).await;
        wait_for_count(&service, 2).await;

        service.shutdown().await.unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn shutdown_ends_the_worker_and_rejects_new_refreshes() {
        let path = temp_path("shutdown");
        let transport = FakeTransport::new([FetchStep::Ready(Ok(state(1, 10)))]);
        let service = ResetCreditService::start_with_events(
            transport.clone(),
            ResetCreditCache::new(path.clone()),
            RecordingEvents::default(),
        )
        .await
        .unwrap();
        service.refresh_now(RefreshReason::Startup).unwrap();
        wait_for_calls(&transport, 1).await;

        service.shutdown().await.unwrap();
        let calls_at_shutdown = transport.calls();
        tokio::time::sleep(Duration::from_millis(30)).await;

        assert!(service.refresh_now(RefreshReason::Manual).is_err());
        assert_eq!(transport.calls(), calls_at_shutdown);
        assert!(service.task.lock().await.is_none());
        let _ = std::fs::remove_file(path);
    }
}
