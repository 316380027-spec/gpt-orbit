use super::supervisor::{wait_or_shutdown, AwaitOutcome};
use super::{
    begin_app_server, AccountSession, AccountState, BackendError, BackendResult, RateLimitCache,
    RateLimitRepository, RateLimitState, RestartBackoff, RpcClient, RpcNotification,
};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::Emitter;
use tokio::sync::{broadcast, mpsc, watch, Mutex};
use tokio::time::MissedTickBehavior;

const UPDATED_EVENT: &str = "rate-limits://updated";
const STATUS_EVENT: &str = "rate-limits://status";
const LOGIN_URL_EVENT: &str = "account://login-url";
const SAFE_FAILURE_MESSAGE: &str = "Unable to refresh Codex quota right now.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshReason {
    Startup,
    Poll,
    LoginCompleted,
    Manual,
    WindowShown,
    Resume,
    SessionUnlocked,
    ResetExpired,
    Tray,
}

pub enum ServiceCommand {
    Refresh(RefreshReason),
    Shutdown,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusPayload {
    status: &'static str,
    message: Option<&'static str>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LoginUrlPayload {
    login_id: String,
    auth_url: String,
}

#[derive(Clone)]
struct AppEvents {
    app: tauri::AppHandle,
    current_tx: watch::Sender<Option<RateLimitState>>,
    current_status_tx: watch::Sender<String>,
}

trait ServiceEvents: Clone + Send + 'static {
    fn state(&self, state: &RateLimitState);
    fn status(&self, status: &'static str, message: Option<&'static str>);
    fn login_url(&self, login_id: String, auth_url: String);
}

impl ServiceEvents for AppEvents {
    fn state(&self, state: &RateLimitState) {
        self.current_tx.send_replace(Some(state.clone()));
        let _ = self.app.emit(UPDATED_EVENT, state);
    }

    fn status(&self, status: &'static str, message: Option<&'static str>) {
        if matches!(
            status,
            "starting" | "live" | "stale" | "offline" | "loginRequired"
        ) {
            self.current_status_tx.send_replace(status.to_string());
        }
        let _ = self
            .app
            .emit(STATUS_EVENT, StatusPayload { status, message });
    }

    fn login_url(&self, login_id: String, auth_url: String) {
        let _ = self
            .app
            .emit(LOGIN_URL_EVENT, LoginUrlPayload { login_id, auth_url });
    }
}

#[derive(Clone)]
struct HeadlessEvents {
    current_tx: watch::Sender<Option<RateLimitState>>,
}

impl ServiceEvents for HeadlessEvents {
    fn state(&self, state: &RateLimitState) {
        self.current_tx.send_replace(Some(state.clone()));
    }

    fn status(&self, _status: &'static str, _message: Option<&'static str>) {}

    fn login_url(&self, _login_id: String, _auth_url: String) {}
}

#[derive(Clone)]
pub struct BackendService {
    command_tx: mpsc::Sender<ServiceCommand>,
    shutdown_complete: watch::Receiver<bool>,
    current_state: watch::Receiver<Option<RateLimitState>>,
    current_status: watch::Receiver<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaBridgeState {
    pub snapshot: Option<RateLimitState>,
    pub status: String,
}

pub struct BackendServiceRegistry {
    inner: Mutex<RegistryInner>,
}

#[derive(Default)]
struct RegistryInner {
    next_generation: u64,
    state: RegistryState,
}

#[derive(Default)]
enum RegistryState {
    #[default]
    Idle,
    Starting {
        generation: u64,
        outcome: watch::Receiver<Option<BackendResult<BackendService>>>,
    },
    Running {
        generation: u64,
        service: BackendService,
    },
}

impl Default for BackendServiceRegistry {
    fn default() -> Self {
        Self {
            inner: Mutex::new(RegistryInner::default()),
        }
    }
}

impl BackendServiceRegistry {
    pub async fn get_or_start<F, Fut>(self: &Arc<Self>, factory: F) -> BackendResult<BackendService>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = BackendResult<BackendService>> + Send + 'static,
    {
        let mut factory = Some(factory);
        let mut outcome = loop {
            let mut inner = self.inner.lock().await;
            match &inner.state {
                RegistryState::Running { service, .. }
                    if !*service.shutdown_complete.borrow() && !service.command_tx.is_closed() =>
                {
                    return Ok(service.clone());
                }
                RegistryState::Running { .. } => {
                    inner.state = RegistryState::Idle;
                }
                RegistryState::Starting { outcome, .. } => break outcome.clone(),
                RegistryState::Idle => {
                    inner.next_generation = inner.next_generation.saturating_add(1);
                    let generation = inner.next_generation;
                    let (outcome_tx, outcome_rx) = watch::channel(None);
                    inner.state = RegistryState::Starting {
                        generation,
                        outcome: outcome_rx.clone(),
                    };
                    let registry = Arc::clone(self);
                    let operation = factory.take().expect("start factory consumed once");
                    let factory_task = tokio::spawn(operation());
                    tokio::spawn(async move {
                        let result = match factory_task.await {
                            Ok(result) => result,
                            Err(_) => Err(BackendError::RpcError("service_start".to_string())),
                        };
                        let running = result.as_ref().ok().cloned();
                        {
                            let mut inner = registry.inner.lock().await;
                            if matches!(
                                inner.state,
                                RegistryState::Starting {
                                    generation: current,
                                    ..
                                } if current == generation
                            ) {
                                inner.state = match &result {
                                    Ok(service) => RegistryState::Running {
                                        generation,
                                        service: service.clone(),
                                    },
                                    Err(_) => RegistryState::Idle,
                                };
                            }
                        }
                        outcome_tx.send_replace(Some(result));
                        if let Some(service) = running {
                            monitor_service_completion(registry, generation, service);
                        }
                    });
                    break outcome_rx;
                }
            }
        };

        loop {
            let result = { outcome.borrow().clone() };
            if let Some(result) = result {
                return result;
            }
            outcome
                .changed()
                .await
                .map_err(|_| BackendError::RpcDisconnected)?;
        }
    }

    async fn clear_generation(&self, generation: u64) {
        let mut inner = self.inner.lock().await;
        if matches!(
            inner.state,
            RegistryState::Running {
                generation: current,
                ..
            } if current == generation
        ) {
            inner.state = RegistryState::Idle;
        }
    }
}

fn monitor_service_completion(
    registry: Arc<BackendServiceRegistry>,
    generation: u64,
    service: BackendService,
) {
    tokio::spawn(async move {
        let mut complete = service.shutdown_complete.clone();
        while !*complete.borrow() {
            if complete.changed().await.is_err() {
                break;
            }
        }
        registry.clear_generation(generation).await;
    });
}

static SERVICE_REGISTRY: OnceLock<Arc<BackendServiceRegistry>> = OnceLock::new();

impl BackendService {
    pub async fn start(
        app: tauri::AppHandle,
        executable: PathBuf,
        cache: RateLimitCache,
    ) -> BackendResult<Self> {
        let registry = Arc::clone(
            SERVICE_REGISTRY.get_or_init(|| Arc::new(BackendServiceRegistry::default())),
        );
        Self::start_with_registry(registry, app, executable, cache).await
    }

    pub async fn start_with_registry(
        registry: Arc<BackendServiceRegistry>,
        app: tauri::AppHandle,
        executable: PathBuf,
        cache: RateLimitCache,
    ) -> BackendResult<Self> {
        registry
            .get_or_start(move || Self::start_unregistered(app, executable, cache))
            .await
    }

    async fn start_unregistered(
        app: tauri::AppHandle,
        executable: PathBuf,
        cache: RateLimitCache,
    ) -> BackendResult<Self> {
        let restored = cache.load().await?;
        let (current_tx, current_state) = watch::channel(restored.clone());
        let (current_status_tx, current_status) = watch::channel("starting".to_string());
        let events = AppEvents {
            app,
            current_tx,
            current_status_tx,
        };
        if let Some(state) = &restored {
            events.state(state);
        }

        let (command_tx, command_rx) = mpsc::channel(32);
        let (shutdown_tx, shutdown_complete) = watch::channel(false);
        let supervisor_events = events.clone();
        let supervisor_cache = cache.clone();
        tokio::spawn(async move {
            supervise(
                command_rx,
                executable,
                supervisor_cache,
                supervisor_events,
                restored,
            )
            .await;
            shutdown_tx.send_replace(true);
        });

        let interval_tx = command_tx.clone();
        let mut interval_shutdown = shutdown_complete.clone();
        tokio::spawn(async move {
            let period = Duration::from_secs(5 * 60);
            let mut interval =
                tokio::time::interval_at(tokio::time::Instant::now() + period, period);
            interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if interval_tx.send(ServiceCommand::Refresh(RefreshReason::Poll)).await.is_err() {
                            return;
                        }
                    }
                    changed = interval_shutdown.changed() => {
                        if changed.is_err() || *interval_shutdown.borrow() {
                            return;
                        }
                    }
                }
            }
        });

        Ok(Self {
            command_tx,
            shutdown_complete,
            current_state,
            current_status,
        })
    }

    pub fn refresh_now(&self, reason: RefreshReason) -> BackendResult<()> {
        match self.command_tx.try_send(ServiceCommand::Refresh(reason)) {
            Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => Ok(()),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(BackendError::RpcDisconnected),
        }
    }

    pub fn current_rate_limits(&self) -> Option<RateLimitState> {
        self.current_state.borrow().clone()
    }

    pub fn current_bridge_state(&self) -> QuotaBridgeState {
        QuotaBridgeState {
            snapshot: self.current_state.borrow().clone(),
            status: self.current_status.borrow().clone(),
        }
    }

    pub async fn shutdown(&self) -> BackendResult<()> {
        if *self.shutdown_complete.borrow() {
            return Ok(());
        }
        let _ = self.command_tx.send(ServiceCommand::Shutdown).await;
        let mut complete = self.shutdown_complete.clone();
        while !*complete.borrow() {
            complete
                .changed()
                .await
                .map_err(|_| BackendError::RpcDisconnected)?;
        }
        Ok(())
    }
}

#[doc(hidden)]
pub async fn run_headless_supervisor(
    executable: impl Into<PathBuf>,
    cache: RateLimitCache,
) -> BackendResult<()> {
    let restored = cache.load().await?;
    let (current_tx, mut current) = watch::channel(restored.clone());
    let events = HeadlessEvents { current_tx };
    let (command_tx, command_rx) = mpsc::channel(4);
    let supervisor = tokio::spawn(supervise(
        command_rx,
        executable.into(),
        cache,
        events,
        restored,
    ));

    while current.borrow().is_none() {
        current
            .changed()
            .await
            .map_err(|_| BackendError::RpcDisconnected)?;
    }
    command_tx
        .send(ServiceCommand::Shutdown)
        .await
        .map_err(|_| BackendError::RpcDisconnected)?;
    supervisor
        .await
        .map_err(|_| BackendError::RpcError("service_headless".to_string()))?;
    Ok(())
}

enum ConnectedEvent {
    Command(Option<ServiceCommand>),
    Notification(Result<RpcNotification, broadcast::error::RecvError>),
    Exited,
    Disconnected,
}

async fn supervise<E: ServiceEvents>(
    mut commands: mpsc::Receiver<ServiceCommand>,
    executable: PathBuf,
    cache: RateLimitCache,
    events: E,
    mut last_state: Option<RateLimitState>,
) {
    let mut backoff = RestartBackoff::default();
    let mut pending_refresh = false;
    'supervisor: loop {
        events.status("starting", None);
        let startup = match begin_app_server(&executable) {
            Ok(startup) => startup,
            Err(_) => {
                publish_offline(&events, last_state.as_ref());
                if matches!(
                    wait_or_shutdown(
                        &mut commands,
                        &mut pending_refresh,
                        tokio::time::sleep(backoff.next_delay()),
                    )
                    .await,
                    AwaitOutcome::Shutdown
                ) {
                    return;
                }
                continue;
            }
        };
        let connection =
            match wait_or_shutdown(&mut commands, &mut pending_refresh, startup.initialize()).await
            {
                AwaitOutcome::Shutdown => {
                    startup.shutdown().await;
                    return;
                }
                AwaitOutcome::Complete(Ok(())) => startup.into_connection(),
                AwaitOutcome::Complete(Err(_)) => {
                    startup.shutdown().await;
                    publish_offline(&events, last_state.as_ref());
                    if matches!(
                        wait_or_shutdown(
                            &mut commands,
                            &mut pending_refresh,
                            tokio::time::sleep(backoff.next_delay()),
                        )
                        .await,
                        AwaitOutcome::Shutdown
                    ) {
                        return;
                    }
                    continue;
                }
            };

        let rpc = connection.rpc.clone();
        let account = AccountSession::new(rpc.clone());
        let repository = RateLimitRepository::new(rpc.clone(), Arc::new(now_unix));
        let mut notifications = rpc.subscribe();

        let account_state =
            wait_or_shutdown(&mut commands, &mut pending_refresh, account.read()).await;
        match account_state {
            AwaitOutcome::Shutdown => {
                connection.shutdown().await;
                return;
            }
            AwaitOutcome::Complete(Ok(account_state)) => {
                backoff.reset();
                match wait_or_shutdown(
                    &mut commands,
                    &mut pending_refresh,
                    apply_account_state(
                        account_state,
                        &account,
                        &repository,
                        &cache,
                        &events,
                        &mut last_state,
                    ),
                )
                .await
                {
                    AwaitOutcome::Shutdown => {
                        connection.shutdown().await;
                        return;
                    }
                    AwaitOutcome::Complete(Err(_)) => {
                        publish_failure(&events, &mut last_state);
                    }
                    AwaitOutcome::Complete(Ok(())) => {}
                }
            }
            AwaitOutcome::Complete(Err(_)) => {
                connection.shutdown().await;
                publish_offline(&events, last_state.as_ref());
                if matches!(
                    wait_or_shutdown(
                        &mut commands,
                        &mut pending_refresh,
                        tokio::time::sleep(backoff.next_delay()),
                    )
                    .await,
                    AwaitOutcome::Shutdown
                ) {
                    return;
                }
                continue;
            }
        }

        let mut connection = connection;
        let mut reconnect = false;
        while !reconnect {
            if pending_refresh {
                pending_refresh = false;
                match wait_or_shutdown(
                    &mut commands,
                    &mut pending_refresh,
                    refresh_and_publish(
                        &repository,
                        &cache,
                        &events,
                        &mut last_state,
                        RefreshReason::Manual,
                    ),
                )
                .await
                {
                    AwaitOutcome::Shutdown => {
                        connection.shutdown().await;
                        return;
                    }
                    AwaitOutcome::Complete(Err(_)) => publish_failure(&events, &mut last_state),
                    AwaitOutcome::Complete(Ok(())) => {}
                }
                continue;
            }
            let event = tokio::select! {
                command = commands.recv() => ConnectedEvent::Command(command),
                notification = notifications.recv() => ConnectedEvent::Notification(notification),
                _ = &mut connection.exit => ConnectedEvent::Exited,
                _ = rpc.wait_disconnected() => ConnectedEvent::Disconnected,
            };
            match event {
                ConnectedEvent::Command(None | Some(ServiceCommand::Shutdown)) => {
                    connection.shutdown().await;
                    return;
                }
                ConnectedEvent::Command(Some(ServiceCommand::Refresh(reason))) => {
                    match wait_or_shutdown(
                        &mut commands,
                        &mut pending_refresh,
                        refresh_and_publish(&repository, &cache, &events, &mut last_state, reason),
                    )
                    .await
                    {
                        AwaitOutcome::Shutdown => {
                            connection.shutdown().await;
                            return;
                        }
                        AwaitOutcome::Complete(Err(_)) => publish_failure(&events, &mut last_state),
                        AwaitOutcome::Complete(Ok(())) => {}
                    }
                }
                ConnectedEvent::Notification(Ok(note)) => {
                    if note.method == "account/updated" {
                        let cleanup = begin_account_invalidation(
                            &repository,
                            &cache,
                            &mut last_state,
                            |state| events.state(state),
                        );
                        match wait_for_owned_cleanup(&mut commands, &mut pending_refresh, cleanup)
                            .await
                        {
                            AwaitOutcome::Shutdown => {
                                connection.shutdown().await;
                                return;
                            }
                            AwaitOutcome::Complete(Err(_)) => {
                                publish_failure(&events, &mut last_state);
                            }
                            AwaitOutcome::Complete(Ok(())) => {}
                        }
                        match wait_or_shutdown(
                            &mut commands,
                            &mut pending_refresh,
                            refresh_account_after_invalidation(
                                &account,
                                &repository,
                                &cache,
                                &events,
                                &mut last_state,
                            ),
                        )
                        .await
                        {
                            AwaitOutcome::Shutdown => {
                                connection.shutdown().await;
                                return;
                            }
                            AwaitOutcome::Complete(Err(_)) => {
                                publish_failure(&events, &mut last_state)
                            }
                            AwaitOutcome::Complete(Ok(())) => {}
                        }
                        continue;
                    }
                    match wait_or_shutdown(
                        &mut commands,
                        &mut pending_refresh,
                        handle_notification(
                            &account,
                            &repository,
                            &cache,
                            &events,
                            &mut last_state,
                            note,
                        ),
                    )
                    .await
                    {
                        AwaitOutcome::Shutdown => {
                            connection.shutdown().await;
                            return;
                        }
                        AwaitOutcome::Complete(Err(_)) => publish_failure(&events, &mut last_state),
                        AwaitOutcome::Complete(Ok(())) => {}
                    }
                }
                ConnectedEvent::Notification(Err(broadcast::error::RecvError::Lagged(_))) => {
                    match wait_or_shutdown(
                        &mut commands,
                        &mut pending_refresh,
                        refresh_and_publish(
                            &repository,
                            &cache,
                            &events,
                            &mut last_state,
                            RefreshReason::Manual,
                        ),
                    )
                    .await
                    {
                        AwaitOutcome::Shutdown => {
                            connection.shutdown().await;
                            return;
                        }
                        AwaitOutcome::Complete(Err(_)) => publish_failure(&events, &mut last_state),
                        AwaitOutcome::Complete(Ok(())) => {}
                    }
                }
                ConnectedEvent::Exited => reconnect = true,
                ConnectedEvent::Notification(Err(broadcast::error::RecvError::Closed))
                | ConnectedEvent::Disconnected => {
                    connection.shutdown().await;
                    publish_offline(&events, last_state.as_ref());
                    if matches!(
                        wait_or_shutdown(
                            &mut commands,
                            &mut pending_refresh,
                            tokio::time::sleep(backoff.next_delay()),
                        )
                        .await,
                        AwaitOutcome::Shutdown
                    ) {
                        return;
                    }
                    continue 'supervisor;
                }
            }
        }
        drop(connection);
        publish_offline(&events, last_state.as_ref());
        if matches!(
            wait_or_shutdown(
                &mut commands,
                &mut pending_refresh,
                tokio::time::sleep(backoff.next_delay()),
            )
            .await,
            AwaitOutcome::Shutdown
        ) {
            return;
        }
    }
}

async fn apply_account_state<E: ServiceEvents>(
    state: AccountState,
    account: &AccountSession<RpcClient>,
    repository: &RateLimitRepository<RpcClient>,
    cache: &RateLimitCache,
    events: &E,
    last_state: &mut Option<RateLimitState>,
) -> BackendResult<()> {
    match state {
        AccountState::Ready { .. } => {
            refresh_and_publish(
                repository,
                cache,
                events,
                last_state,
                RefreshReason::Startup,
            )
            .await
        }
        AccountState::LoginRequired => {
            events.status("loginRequired", None);
            if let AccountState::LoginPending { login_id, auth_url } =
                account.begin_browser_login().await?
            {
                events.login_url(login_id, auth_url);
            }
            Ok(())
        }
        AccountState::LoginPending { login_id, auth_url } => {
            events.status("loginRequired", None);
            events.login_url(login_id, auth_url);
            Ok(())
        }
    }
}

async fn handle_notification<E: ServiceEvents>(
    account: &AccountSession<RpcClient>,
    repository: &RateLimitRepository<RpcClient>,
    cache: &RateLimitCache,
    events: &E,
    last_state: &mut Option<RateLimitState>,
    note: RpcNotification,
) -> BackendResult<()> {
    if let Some(state) = repository.apply_notification(&note).await? {
        publish_live(cache, events, last_state, state).await;
        return Ok(());
    }
    if let Some(state) = account.handle_notification(&note).await? {
        return apply_account_state(state, account, repository, cache, events, last_state).await;
    }
    Ok(())
}

async fn refresh_account_after_invalidation<E: ServiceEvents>(
    account: &AccountSession<RpcClient>,
    repository: &RateLimitRepository<RpcClient>,
    cache: &RateLimitCache,
    events: &E,
    last_state: &mut Option<RateLimitState>,
) -> BackendResult<()> {
    let state = account.read().await?;
    apply_account_state(state, account, repository, cache, events, last_state).await
}

#[cfg(test)]
async fn invalidate_account_state<T, F>(
    repository: &RateLimitRepository<T>,
    cache: &RateLimitCache,
    last_state: &mut Option<RateLimitState>,
    publish: F,
) -> BackendResult<()>
where
    T: super::RpcTransport,
    F: FnOnce(&RateLimitState),
{
    let cleanup = begin_account_invalidation(repository, cache, last_state, publish);
    cleanup
        .await
        .map_err(|_| BackendError::RpcError("cache_cleanup".to_string()))?
}

fn begin_account_invalidation<T, F>(
    repository: &RateLimitRepository<T>,
    cache: &RateLimitCache,
    last_state: &mut Option<RateLimitState>,
    publish: F,
) -> tokio::task::JoinHandle<BackendResult<()>>
where
    T: super::RpcTransport,
    F: FnOnce(&RateLimitState),
{
    repository.reset();
    let empty = empty_rate_limit_state();
    *last_state = Some(empty.clone());
    publish(&empty);
    let cache = cache.clone();
    tokio::spawn(async move {
        let store = cache.store(&empty);
        replace_then_clear_cache(&cache, store).await
    })
}

async fn replace_then_clear_cache<F>(cache: &RateLimitCache, store: F) -> BackendResult<()>
where
    F: std::future::Future<Output = BackendResult<()>>,
{
    let store_result = store.await;
    let clear_result = cache.clear().await;
    match (store_result, clear_result) {
        (_, Err(error)) | (Err(error), Ok(())) => Err(error),
        (Ok(()), Ok(())) => Ok(()),
    }
}

async fn wait_for_owned_cleanup(
    commands: &mut mpsc::Receiver<ServiceCommand>,
    pending_refresh: &mut bool,
    mut cleanup: tokio::task::JoinHandle<BackendResult<()>>,
) -> AwaitOutcome<BackendResult<()>> {
    match wait_or_shutdown(commands, pending_refresh, &mut cleanup).await {
        AwaitOutcome::Shutdown => {
            let _ = cleanup.await;
            AwaitOutcome::Shutdown
        }
        AwaitOutcome::Complete(result) => AwaitOutcome::Complete(
            result.unwrap_or_else(|_| Err(BackendError::RpcError("cache_cleanup".to_string()))),
        ),
    }
}

fn empty_rate_limit_state() -> RateLimitState {
    RateLimitState {
        five_hour: None,
        weekly: None,
        other: Vec::new(),
        plan_type: None,
        reached_type: None,
        fetched_at: now_unix(),
        source: super::RateLimitSource::Cache,
        stale: true,
    }
}

async fn refresh_and_publish<E: ServiceEvents>(
    repository: &RateLimitRepository<RpcClient>,
    cache: &RateLimitCache,
    events: &E,
    last_state: &mut Option<RateLimitState>,
    _reason: RefreshReason,
) -> BackendResult<()> {
    events.status("refreshing", None);
    let state = repository.refresh().await?;
    publish_live(cache, events, last_state, state).await;
    Ok(())
}

async fn publish_live<E: ServiceEvents>(
    cache: &RateLimitCache,
    events: &E,
    last_state: &mut Option<RateLimitState>,
    state: RateLimitState,
) {
    if cache.store(&state).await.is_err() {
        tracing::warn!(category = "cache_write_failed");
    }
    events.state(&state);
    events.status("live", None);
    *last_state = Some(state);
}

fn publish_failure<E: ServiceEvents>(events: &E, last_state: &mut Option<RateLimitState>) {
    if let Some(state) = last_state {
        state.stale = true;
        events.state(state);
    }
    events.status("offline", Some(SAFE_FAILURE_MESSAGE));
}

fn publish_offline<E: ServiceEvents>(events: &E, state: Option<&RateLimitState>) {
    if let Some(state) = state {
        let mut stale = state.clone();
        stale.stale = true;
        events.state(&stale);
    }
    events.status("offline", Some(SAFE_FAILURE_MESSAGE));
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        empty_rate_limit_state, invalidate_account_state, replace_then_clear_cache,
        wait_for_owned_cleanup, wait_or_shutdown, AwaitOutcome, BackendError, BackendService,
        BackendServiceRegistry, RefreshReason, ServiceCommand,
    };
    use crate::backend::{BackendResult, RateLimitCache, RateLimitRepository, RpcTransport};
    use serde_json::{json, Value};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use tokio::sync::{mpsc, watch, Notify, Semaphore};

    #[tokio::test]
    async fn concurrent_refresh_is_coalesced() {
        let calls = Arc::new(AtomicUsize::new(0));
        let operation_calls = Arc::clone(&calls);
        let started = Arc::new(Notify::new());
        let operation_started = Arc::clone(&started);
        let release = Arc::new(Semaphore::new(0));
        let operation_release = Arc::clone(&release);
        let (tx, mut rx) = mpsc::channel(16);
        let worker = tokio::spawn(async move {
            let mut pending = false;
            let outcome = wait_or_shutdown(&mut rx, &mut pending, async move {
                operation_calls.fetch_add(1, Ordering::SeqCst);
                operation_started.notify_one();
                operation_release.acquire().await.unwrap().forget();
            })
            .await;
            (outcome, pending)
        });

        started.notified().await;
        for reason in [
            RefreshReason::Tray,
            RefreshReason::Resume,
            RefreshReason::Poll,
        ] {
            tx.send(ServiceCommand::Refresh(reason)).await.unwrap();
        }
        release.add_permits(1);
        let (outcome, pending) = worker.await.unwrap();

        assert!(matches!(outcome, AwaitOutcome::Complete(())));
        assert!(
            pending,
            "concurrent refreshes must collapse into one pending read"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    fn test_service(command_tx: mpsc::Sender<ServiceCommand>) -> BackendService {
        let (_, shutdown_complete) = watch::channel(false);
        BackendService {
            command_tx,
            shutdown_complete,
            current_state: watch::channel(None).1,
            current_status: watch::channel("starting".to_string()).1,
        }
    }

    fn running_test_service() -> BackendService {
        let (command_tx, mut command_rx) = mpsc::channel(4);
        let (shutdown_tx, shutdown_complete) = watch::channel(false);
        tokio::spawn(async move {
            while let Some(command) = command_rx.recv().await {
                if matches!(command, ServiceCommand::Shutdown) {
                    break;
                }
            }
            shutdown_tx.send_replace(true);
        });
        BackendService {
            command_tx,
            shutdown_complete,
            current_state: watch::channel(None).1,
            current_status: watch::channel("starting".to_string()).1,
        }
    }

    #[tokio::test]
    async fn concurrent_registry_start_returns_one_production_handle() {
        let registry = Arc::new(BackendServiceRegistry::default());
        let starts = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(Semaphore::new(0));
        let start = |registry: Arc<BackendServiceRegistry>,
                     starts: Arc<AtomicUsize>,
                     release: Arc<Semaphore>| async move {
            registry
                .get_or_start(move || async move {
                    starts.fetch_add(1, Ordering::SeqCst);
                    release.acquire().await.unwrap().forget();
                    let (tx, _rx) = mpsc::channel(1);
                    Ok(test_service(tx))
                })
                .await
        };

        let first = tokio::spawn(start(
            Arc::clone(&registry),
            Arc::clone(&starts),
            Arc::clone(&release),
        ));
        tokio::task::yield_now().await;
        let second = tokio::spawn(start(
            Arc::clone(&registry),
            Arc::clone(&starts),
            Arc::clone(&release),
        ));
        tokio::task::yield_now().await;
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        release.add_permits(1);

        let first = first.await.unwrap().unwrap();
        let second = second.await.unwrap().unwrap();
        assert!(first.command_tx.same_channel(&second.command_tx));
        assert_eq!(starts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn registry_restarts_after_normal_shutdown() {
        let registry = Arc::new(BackendServiceRegistry::default());
        let first = registry
            .get_or_start(|| async { Ok(running_test_service()) })
            .await
            .unwrap();
        first.shutdown().await.unwrap();

        let second = tokio::time::timeout(
            std::time::Duration::from_millis(250),
            registry.get_or_start(|| async { Ok(running_test_service()) }),
        )
        .await
        .expect("registry remained permanently started")
        .unwrap();

        assert!(!first.command_tx.same_channel(&second.command_tx));
        second.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn registry_restarts_after_supervisor_ends_early() {
        let registry = Arc::new(BackendServiceRegistry::default());
        let (command_tx, _command_rx) = mpsc::channel(1);
        let (shutdown_tx, shutdown_complete) = watch::channel(false);
        let first = registry
            .get_or_start(move || async move {
                Ok(BackendService {
                    command_tx,
                    shutdown_complete,
                    current_state: watch::channel(None).1,
                    current_status: watch::channel("starting".to_string()).1,
                })
            })
            .await
            .unwrap();
        shutdown_tx.send_replace(true);
        tokio::task::yield_now().await;

        let second = registry
            .get_or_start(|| async { Ok(running_test_service()) })
            .await
            .unwrap();

        assert!(!first.command_tx.same_channel(&second.command_tx));
        second.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn registry_retries_after_start_failure() {
        let registry = Arc::new(BackendServiceRegistry::default());
        let failed = registry
            .get_or_start(|| async {
                Err(BackendError::RpcError(
                    "controlled_start_failure".to_string(),
                ))
            })
            .await;
        assert!(failed.is_err());

        let service = registry
            .get_or_start(|| async { Ok(running_test_service()) })
            .await
            .unwrap();

        service.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn registry_recovers_when_start_factory_panics() {
        let registry = Arc::new(BackendServiceRegistry::default());
        let failed = tokio::time::timeout(
            std::time::Duration::from_millis(250),
            registry.get_or_start(|| async {
                panic!("controlled_start_panic");
                #[allow(unreachable_code)]
                Ok(running_test_service())
            }),
        )
        .await
        .expect("panicking start left current waiters pending");
        let failed = match failed {
            Err(error) => error,
            Ok(_) => panic!("panicking start unexpectedly succeeded"),
        };
        assert!(matches!(failed, BackendError::RpcError(message) if message == "service_start"));

        let service = tokio::time::timeout(
            std::time::Duration::from_millis(250),
            registry.get_or_start(|| async { Ok(running_test_service()) }),
        )
        .await
        .expect("panicking generation left registry permanently Starting")
        .unwrap();

        service.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn busy_healthy_refresh_is_coalesced_not_reported_disconnected() {
        let (tx, _rx) = mpsc::channel(1);
        tx.try_send(ServiceCommand::Refresh(RefreshReason::Poll))
            .unwrap();
        let service = test_service(tx);

        assert!(service.refresh_now(RefreshReason::Manual).is_ok());
    }

    #[tokio::test]
    async fn current_snapshot_is_retained_for_late_query() {
        let (tx, _rx) = mpsc::channel(1);
        let (_, shutdown_complete) = watch::channel(false);
        let expected = empty_rate_limit_state();
        let (_, current_state) = watch::channel(Some(expected.clone()));
        let service = BackendService {
            command_tx: tx,
            shutdown_complete,
            current_state,
            current_status: watch::channel("loginRequired".to_string()).1,
        };

        assert_eq!(service.current_rate_limits(), Some(expected.clone()));
        let bridge_state = service.current_bridge_state();
        assert_eq!(bridge_state.snapshot, Some(expected));
        assert_eq!(bridge_state.status, "loginRequired");
    }

    #[derive(Clone)]
    struct QuotaTransport;

    #[async_trait::async_trait]
    impl RpcTransport for QuotaTransport {
        async fn request_value(
            &self,
            _method: &'static str,
            _params: Option<Value>,
        ) -> BackendResult<Value> {
            Ok(json!({
                "rateLimits": {
                    "primary": {"usedPercent": 10, "windowDurationMins": 300},
                    "secondary": {"usedPercent": 20, "windowDurationMins": 10080}
                }
            }))
        }
    }

    #[tokio::test]
    async fn account_change_clears_repository_cache_and_published_quota_before_read() {
        let repository = RateLimitRepository::new(QuotaTransport, Arc::new(|| 1_700_000_000));
        let live = repository.refresh().await.unwrap();
        let path = std::env::temp_dir().join(format!(
            "codex-orbit-account-change-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let cache = RateLimitCache::new(path.clone());
        cache.store(&live).await.unwrap();
        let mut last_state = Some(live);
        let published = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&published);

        invalidate_account_state(&repository, &cache, &mut last_state, move |state| {
            captured.lock().unwrap().push(state.clone());
        })
        .await
        .unwrap();

        assert_eq!(repository.current().await, None);
        assert_eq!(cache.load().await.unwrap(), None);
        assert!(last_state.as_ref().unwrap().five_hour.is_none());
        assert!(published
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .five_hour
            .is_none());
    }

    #[tokio::test]
    async fn account_change_deletes_old_cache_when_empty_store_fails() {
        let path = std::env::temp_dir().join(format!(
            "codex-orbit-account-store-failure-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let cache = RateLimitCache::new(path.clone());
        cache.store(&empty_rate_limit_state()).await.unwrap();

        let result = replace_then_clear_cache(&cache, async {
            Err(BackendError::RpcError(
                "controlled_store_failure".to_string(),
            ))
        })
        .await;

        assert!(result.is_err());
        assert_eq!(cache.load().await.unwrap(), None);
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn shutdown_waits_for_owned_account_cleanup_to_delete_old_cache() {
        let path = std::env::temp_dir().join(format!(
            "codex-orbit-account-shutdown-cleanup-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let cache = RateLimitCache::new(path.clone());
        cache.store(&empty_rate_limit_state()).await.unwrap();
        let release = Arc::new(Semaphore::new(0));
        let cleanup_release = Arc::clone(&release);
        let cleanup_cache = cache.clone();
        let cleanup = tokio::spawn(async move {
            cleanup_release.acquire().await.unwrap().forget();
            cleanup_cache.clear().await
        });
        let (tx, mut rx) = mpsc::channel(1);
        let waiter = tokio::spawn(async move {
            let mut pending_refresh = false;
            wait_for_owned_cleanup(&mut rx, &mut pending_refresh, cleanup).await
        });

        tx.send(ServiceCommand::Shutdown).await.unwrap();
        tokio::task::yield_now().await;
        assert!(
            !waiter.is_finished(),
            "shutdown skipped the cleanup transaction"
        );
        release.add_permits(1);

        assert!(matches!(waiter.await.unwrap(), AwaitOutcome::Shutdown));
        assert_eq!(cache.load().await.unwrap(), None);
        assert!(!path.exists());
    }
}
