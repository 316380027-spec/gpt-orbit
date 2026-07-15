# Codex Orbit Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Rust backend that owns one `codex app-server`, completes the official ChatGPT login flow, reads and merges quota updates, preserves a safe cache, and reconnects without duplicate processes.

**Architecture:** A single supervisor task owns the child process and creates a JSONL RPC client for each connection. `AccountSession` drives authentication; `RateLimitRepository` owns the last full raw rate-limit snapshot, merges sparse notifications, normalizes display data, and writes only normalized fields to cache.

**Tech Stack:** Tauri 2, Rust, Tokio, Serde/serde_json, thiserror, tracing, async-trait, Rust unit and integration tests.

## Global Constraints

- Target Windows 11; run `codex app-server --listen stdio://` with no visible console window.
- Wire transport is newline-delimited JSON and omits `"jsonrpc":"2.0"`.
- Send one `initialize` request and then one `initialized` notification per connection before account requests.
- Never read browser cookies, request copied tokens, persist login URLs, or log email/token/raw responses.
- Cache only normalized quota fields; restored cache always has `source = cache` and `stale = true`.
- Retry child exits after 1, 2, 4, 8, then at most 30 seconds; at most one managed child may exist.
- Treat `account/rateLimits/updated` as a sparse patch: absent keys retain old values; explicit `null` replaces old values.

---

## File Map

- `src-tauri/src/backend/error.rs`: safe error taxonomy.
- `src-tauri/src/backend/protocol.rs`: internal snapshot and notification types.
- `src-tauri/src/backend/rpc.rs`: JSONL framing, request correlation, notifications, handshake.
- `src-tauri/src/backend/process.rs`: hidden child creation and stdio reader.
- `src-tauri/src/backend/account.rs`: account/read and browser login state machine.
- `src-tauri/src/backend/rate_limits.rs`: full reads, sparse merge, validation and normalization.
- `src-tauri/src/backend/cache.rs`: normalized on-disk cache.
- `src-tauri/src/backend/supervisor.rs`: one-owner lifecycle and restart backoff.
- `src-tauri/src/backend/service.rs`: refresh scheduling and Tauri event bridge.
- `src-tauri/src/bin/mock_app_server.rs`: deterministic JSONL fixture process.
- `src-tauri/tests/app_server_flow.rs`: end-to-end process/RPC/login/quota tests.

### Task 1: JSONL RPC and initialization

**Files:**
- Create: `src-tauri/src/backend/error.rs`
- Create: `src-tauri/src/backend/protocol.rs`
- Create: `src-tauri/src/backend/rpc.rs`
- Modify: `src-tauri/src/backend/mod.rs`

**Interfaces:**

```rust
pub type BackendResult<T> = Result<T, BackendError>;

#[derive(Clone, Debug)]
pub struct RpcNotification { pub method: String, pub params: serde_json::Value }

#[derive(Clone)]
pub struct RpcClient { /* writer queue, pending map, notification sender */ }

impl RpcClient {
    pub fn new<W: tokio::io::AsyncWrite + Unpin + Send + 'static>(writer: W) -> Self;
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<RpcNotification>;
    pub async fn request<P: serde::Serialize, R: serde::de::DeserializeOwned>(
        &self, method: &'static str, params: Option<P>
    ) -> BackendResult<R>;
    pub async fn notify<P: serde::Serialize>(
        &self, method: &'static str, params: Option<P>
    ) -> BackendResult<()>;
    pub async fn accept_line(&self, line: &str) -> BackendResult<()>;
    pub async fn disconnect(&self);
}

pub async fn initialize(client: &RpcClient, version: &str) -> BackendResult<()>;
```

- [ ] Write tests named `matches_out_of_order_responses`, `dispatches_idless_notification`, `disconnect_fails_pending_requests`, `bad_json_does_not_poison_next_line`, and `initialized_is_sent_only_after_initialize_response`. Use `tokio::io::duplex`, assert IDs are `1, 2`, feed responses in `2, 1` order, and assert both futures receive the correct result.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml backend::rpc::tests`; expect compilation failures because `RpcClient` is absent.
- [ ] Implement a single writer task, `AtomicU64` IDs, `Mutex<HashMap<u64, oneshot::Sender<BackendResult<Value>>>>`, a 15-second request timeout, and broadcast notifications. Bad JSON returns `InvalidMessage`; the caller logs the category and continues. `disconnect()` drains every pending sender with `RpcDisconnected`.
- [ ] Implement handshake wire messages exactly as `{"method":"initialize","id":1,"params":{"clientInfo":{"name":"codex_orbit","title":"Codex Orbit","version":"..."}}}` followed, after success, by `{"method":"initialized","params":{}}`. Do not add a JSON-RPC version field or experimental capability.
- [ ] Re-run the RPC test command; expect all five tests to pass.
- [ ] Commit with `git add src-tauri/src/backend && git commit -m "feat: add app server jsonl rpc client"`.

### Task 2: Child process and ChatGPT browser login

**Files:**
- Create: `src-tauri/src/backend/process.rs`
- Create: `src-tauri/src/backend/account.rs`

**Interfaces:**

```rust
pub struct AppServerConnection {
    pub rpc: RpcClient,
    pub exit: tokio::sync::oneshot::Receiver<std::process::ExitStatus>,
}
pub async fn spawn_app_server(executable: &std::path::Path) -> BackendResult<AppServerConnection>;

#[async_trait::async_trait]
pub trait RpcTransport: Clone + Send + Sync + 'static {
    async fn request_value(&self, method: &'static str, params: Option<Value>) -> BackendResult<Value>;
}

#[derive(Clone, Debug, PartialEq)]
pub enum AccountState {
    Ready { plan_type: Option<String> },
    LoginRequired,
    LoginPending { login_id: String, auth_url: String },
}

pub struct AccountSession<T: RpcTransport>;
impl<T: RpcTransport> AccountSession<T> {
    pub fn new(rpc: T) -> Self;
    pub async fn read(&self) -> BackendResult<AccountState>;
    pub async fn begin_browser_login(&self) -> BackendResult<AccountState>;
    pub async fn handle_notification(&self, note: &RpcNotification)
        -> BackendResult<Option<AccountState>>;
}
```

- [ ] Write tests `spawns_stdio_app_server`, `chatgpt_account_is_ready_without_retaining_email`, `starts_only_one_pending_login`, `matching_login_completion_rereads_account`, and `old_login_completion_is_ignored`. The mock transport must count method calls and retain no email.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml backend::process::tests backend::account::tests`; expect missing-type failures.
- [ ] Spawn `codex app-server --listen stdio://`, pipe stdin/stdout/stderr, and apply Windows `CREATE_NO_WINDOW` (`0x08000000`). Keep stderr separate from protocol stdout and log only lifecycle/error categories.
- [ ] Make `read()` call `account/read` with `{"refreshToken":false}`. A ChatGPT account becomes `Ready`; `account == null && requiresOpenaiAuth` becomes `LoginRequired`; API-key/local-provider states do not masquerade as ChatGPT quota availability.
- [ ] Make `begin_browser_login()` call `account/login/start` with `{"type":"chatgpt","useHostedLoginSuccessPage":true,"appBrand":"codex"}`. Keep one in-memory pending `{loginId, authUrl}`, never log or persist the URL, and wait for matching `account/login/completed` before re-reading the account.
- [ ] Re-run the test command; expect all five tests to pass.
- [ ] Commit with `git add src-tauri/src/backend/process.rs src-tauri/src/backend/account.rs && git commit -m "feat: manage codex account login"`.

### Task 3: Rate-limit repository and sparse updates

**Files:**
- Create: `src-tauri/src/backend/rate_limits.rs`

**Interfaces:**

```rust
pub fn merge_sparse(base: &mut serde_json::Value, patch: &serde_json::Value);
pub fn normalize_rate_limits(
    raw: &Value, plan_type: Option<String>, fetched_at: i64, source: RateLimitSource
) -> BackendResult<RateLimitState>;

pub struct RateLimitRepository<T: RpcTransport>;
impl<T: RpcTransport> RateLimitRepository<T> {
    pub fn new(rpc: T, now: std::sync::Arc<dyn Fn() -> i64 + Send + Sync>) -> Self;
    pub async fn refresh(&self) -> BackendResult<RateLimitState>;
    pub async fn apply_notification(&self, note: &RpcNotification)
        -> BackendResult<Option<RateLimitState>>;
    pub async fn current(&self) -> Option<RateLimitState>;
}
```

- [ ] Write tests `sparse_patch_keeps_missing_weekly`, `explicit_null_clears_weekly`, `classifies_300_as_five_hour`, `classifies_10080_as_weekly`, `clamps_used_percent`, `rejects_non_numeric_usage`, and `notification_without_read_baseline_refetches`.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml backend::rate_limits::tests`; expect missing-function failures.
- [ ] Implement recursive object merge: recurse for object/object; insert missing keys; otherwise replace, including explicit `null`. Keep the last full `result.rateLimits` JSON only in memory. `refresh()` calls `account/rateLimits/read`; notifications consume only `params.rateLimits`.
- [ ] Normalize finite positive durations: 240–360 minutes is `fiveHour`, 9360–10800 is `weekly`; only unclassified top-level `primary`/`secondary` may fall back to those names. Clamp finite `usedPercent` to 0–100 and compute `remainingPercent = 100 - usedPercent`; invalid reset timestamps become `null`.
- [ ] Re-run the test command; expect seven passing tests.
- [ ] Commit with `git add src-tauri/src/backend/rate_limits.rs && git commit -m "feat: merge and normalize codex quotas"`.

### Task 4: Safe cache and supervised recovery

**Files:**
- Create: `src-tauri/src/backend/cache.rs`
- Create: `src-tauri/src/backend/supervisor.rs`
- Create: `src-tauri/src/backend/service.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**

```rust
pub struct RateLimitCache { path: std::path::PathBuf }
impl RateLimitCache {
    pub fn new(path: std::path::PathBuf) -> Self;
    pub async fn load(&self) -> BackendResult<Option<RateLimitState>>;
    pub async fn store(&self, state: &RateLimitState) -> BackendResult<()>;
}

#[derive(Default)]
pub struct RestartBackoff { failures: usize }
impl RestartBackoff {
    pub fn next_delay(&mut self) -> std::time::Duration;
    pub fn reset(&mut self);
}

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

pub enum ServiceCommand { Refresh(RefreshReason), Shutdown }
pub struct BackendService { command_tx: tokio::sync::mpsc::Sender<ServiceCommand> }
impl BackendService {
    pub async fn start(app: tauri::AppHandle, executable: PathBuf, cache: RateLimitCache)
        -> BackendResult<Self>;
    pub fn refresh_now(&self, reason: RefreshReason) -> BackendResult<()>;
    pub async fn shutdown(&self) -> BackendResult<()>;
}
```

- [ ] Write tests `restored_cache_is_always_stale`, `cache_has_no_sensitive_keys`, `backoff_is_1_2_4_8_30_capped`, `stable_connection_resets_backoff`, `concurrent_start_spawns_one_child`, and `concurrent_refresh_is_coalesced`.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml backend::cache::tests backend::supervisor::tests backend::service::tests`; expect missing-type failures.
- [ ] Store only `RateLimitState` at `app_data_dir()/rate-limits-v1.json`; force loaded data to `source=Cache, stale=true`. Treat corrupt cache as no cache and log only `cache_invalid`.
- [ ] Give one supervisor task sole ownership of `Option<Child>`. Startup order is cache publish → spawn → reader → initialize → account/read → login or rate-limit read. On exit: disconnect pending RPCs, publish stale/offline state, wait 1/2/4/8/30 seconds, then create a new process and handshake. Reset backoff after a successful account read.
- [ ] Add one five-minute Tokio interval with `MissedTickBehavior::Skip`; send `ServiceCommand::Refresh(RefreshReason::Poll)` on each tick. All refresh reasons trigger immediate reads, coalesced behind one in-flight refresh. Shutdown stops the interval, kills and waits for the child.
- [ ] Emit only `rate-limits://updated`, `rate-limits://status`, and `account://login-url`. The login payload contains only `loginId` and `authUrl`; backend failures use the status event's user-safe `message` and never expose raw errors.
- [ ] Re-run the test command; expect six passing tests.
- [ ] Commit with `git add src-tauri/src/backend src-tauri/src/lib.rs && git commit -m "feat: supervise and cache quota service"`.

### Task 5: Process-level integration tests

**Files:**
- Create: `src-tauri/src/bin/mock_app_server.rs`
- Create: `src-tauri/tests/app_server_flow.rs`

**Interfaces:** The fixture accepts `MOCK_APP_SERVER_SCENARIO=logged_in|login_required|sparse_update|bad_json|exit_once` and communicates only through stdin/stdout JSONL.

- [ ] Write integration tests proving handshake order, logged-in dual-window read, browser-login completion, sparse update retention, recovery after malformed JSON, and restart after one process exit with `max_simultaneous_children == 1`.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --test app_server_flow`; expect failures because the fixture binary is absent.
- [ ] Implement the fixture so account requests before `initialized` return `{"error":{"code":-32002,"message":"Not initialized"}}`; make `exit_once` persist one marker in its temporary scenario directory so only the first child exits.
- [ ] Run `cargo fmt --manifest-path src-tauri/Cargo.toml --check`, `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`, and `cargo test --manifest-path src-tauri/Cargo.toml --all-targets`; expect exit code 0, no warnings, and all tests passing.
- [ ] Run `codex --version` and `codex app-server generate-json-schema --out target/codex-app-server-schema`; expect exit code 0 and generated schema matching the installed Codex version.
- [ ] Commit with `git add src-tauri/src/bin/mock_app_server.rs src-tauri/tests/app_server_flow.rs && git commit -m "test: cover app server quota lifecycle"`.

## Protocol Risks

- GitHub `main` may differ from the installed CLI. Generate schema with the user's installed `codex` and tolerate unknown response fields.
- `account/rateLimits/updated` is explicitly sparse; never deserialize it as a complete snapshot. Missing and explicit null have different meanings.
- Error `-32001` (`Server overloaded; retry later.`) is retryable on the same connection; it must not spawn another child.
- `authUrl`, email, raw responses, and stderr may contain sensitive data; never include them in normal logs or cache.
- On `account/updated`, discard the prior raw quota baseline and perform a full account and quota read to avoid showing a previous workspace's quota.
- `CodexNotFound` is a terminal diagnostic until the executable path changes; do not place it in the child-exit restart loop.
- Official protocol reference: https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md
