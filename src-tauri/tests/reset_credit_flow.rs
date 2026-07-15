use codex_orbit_lib::backend::{
    BackendError, RefreshReason, ResetCreditCache, ResetCreditClient, ResetCreditService,
    ResetCreditServiceEvents, ResetCreditState, ResetCreditTransport,
};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::Url;

const SYNTHETIC_TOKEN: &str = "fixture-token-never-log";
const SYNTHETIC_ACCOUNT: &str = "fixture-account-never-log";
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

struct IsolatedCodexHome {
    root: PathBuf,
    auth_path: PathBuf,
    cache: ResetCreditCache,
}

impl IsolatedCodexHome {
    fn new() -> Self {
        let unique = format!(
            "{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock before epoch")
                .as_nanos(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        );
        let root = std::env::temp_dir().join(format!("gpt-orbit-reset-credit-{unique}"));
        let codex_home = root.join("codex-home");
        fs::create_dir_all(&codex_home).expect("create isolated CODEX_HOME");
        let auth_path = codex_home.join("auth.json");
        fs::write(
            &auth_path,
            serde_json::to_vec(&json!({
                "tokens": {
                    "access_token": SYNTHETIC_TOKEN,
                    "account_id": SYNTHETIC_ACCOUNT
                }
            }))
            .expect("encode synthetic auth"),
        )
        .expect("write isolated auth file");
        let cache = ResetCreditCache::new(root.join("reset-credits-v1.json"));
        Self {
            root,
            auth_path,
            cache,
        }
    }
}

impl Drop for IsolatedCodexHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct FakeResetCreditServer {
    child: Child,
    state_dir: PathBuf,
    endpoint: Url,
}

impl FakeResetCreditServer {
    fn start(home: &IsolatedCodexHome, scenario: &str) -> Self {
        let root = workspace_root();
        let script = root.join("tests/fake-reset-credit-server/server.mjs");
        let state_dir = home.root.join(format!("server-{scenario}"));
        fs::create_dir_all(&state_dir).expect("create fake server state directory");
        let mut child = Command::new("node")
            .arg(script)
            .env("GPT_ORBIT_RESET_SCENARIO", scenario)
            .env("GPT_ORBIT_RESET_STATE_DIR", &state_dir)
            .env("CODEX_HOME", home.auth_path.parent().unwrap())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start loopback reset-credit fixture");

        let port_path = state_dir.join("port");
        let port = (0..200)
            .find_map(|_| {
                if let Ok(port) = fs::read_to_string(&port_path) {
                    return port.trim().parse::<u16>().ok();
                }
                if child.try_wait().ok().flatten().is_some() {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(10));
                None
            })
            .unwrap_or_else(|| panic!("fake scenario {scenario} did not bind loopback"));
        let endpoint = Url::parse(&format!("http://127.0.0.1:{port}/reset-credits"))
            .expect("fixture endpoint URL");
        Self {
            child,
            state_dir,
            endpoint,
        }
    }

    fn client(&self, home: &IsolatedCodexHome) -> ResetCreditClient {
        ResetCreditClient::with_loopback_endpoint(self.endpoint.clone(), home.auth_path.clone())
            .expect("construct loopback-only reset-credit client")
    }

    fn request_count(&self) -> u64 {
        fs::read_to_string(self.state_dir.join("request-count"))
            .expect("fake server request-count exists")
            .trim()
            .parse()
            .expect("fake server request-count is numeric")
    }

    fn observed_header_names(&self) -> Vec<String> {
        serde_json::from_slice(
            &fs::read(self.state_dir.join("header-names.json"))
                .expect("fake server header-name record exists"),
        )
        .expect("fake server records a JSON array of header names")
    }
}

impl Drop for FakeResetCreditServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn assert_safe_error(error: &impl std::fmt::Debug) {
    let rendered = format!("{error:?}");
    assert!(!rendered.contains(SYNTHETIC_TOKEN));
    assert!(!rendered.contains(SYNTHETIC_ACCOUNT));
    assert!(!rendered.contains("access_token"));
    assert!(!rendered.contains("account_id"));
    assert!(!rendered.contains('{'));
}

async fn fetch_scenario(
    home: &IsolatedCodexHome,
    scenario: &str,
) -> (
    FakeResetCreditServer,
    codex_orbit_lib::backend::BackendResult<ResetCreditState>,
) {
    let server = FakeResetCreditServer::start(home, scenario);
    let result = server.client(home).fetch().await;
    (server, result)
}

#[derive(Clone, Default)]
struct RecordingServiceEvents {
    states: Arc<Mutex<Vec<ResetCreditState>>>,
}

impl ResetCreditServiceEvents for RecordingServiceEvents {
    fn state(&self, state: &ResetCreditState) {
        self.states.lock().unwrap().push(state.clone());
    }
}

impl RecordingServiceEvents {
    async fn wait_for(&self, expected_count: u32, expected_stale: bool) -> ResetCreditState {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let latest = { self.states.lock().unwrap().last().cloned() };
                if let Some(state) = latest {
                    if state.available_count == Some(expected_count)
                        && state.stale == expected_stale
                    {
                        break state;
                    }
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("production reset-credit service event timed out")
    }
}

async fn wait_for_service_state(
    service: &ResetCreditService,
    expected_count: u32,
    expected_stale: bool,
) -> ResetCreditState {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(current) = service.current() {
                if current.available_count == Some(expected_count)
                    && current.stale == expected_stale
                {
                    break current;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("production reset-credit service state timed out")
}

#[tokio::test]
async fn production_service_keeps_stale_state_and_cache_until_loopback_recovery() {
    let home = IsolatedCodexHome::new();
    let server = FakeResetCreditServer::start(&home, "service-sequence");
    let events = RecordingServiceEvents::default();
    let service = ResetCreditService::start_with_events(
        server.client(&home),
        home.cache.clone(),
        events.clone(),
    )
    .await
    .expect("start production reset-credit service with loopback transport");
    service.refresh_now(RefreshReason::Startup).unwrap();

    let live = wait_for_service_state(&service, 3, false).await;
    assert_eq!(events.wait_for(3, false).await, live);
    let cached_live = home.cache.load().await.unwrap().unwrap();
    assert_eq!(cached_live.available_count, Some(3));

    service.refresh_now(RefreshReason::Manual).unwrap();
    let stale = wait_for_service_state(&service, 3, true).await;
    assert_eq!(events.wait_for(3, true).await, stale);
    let cached_after_failure = home.cache.load().await.unwrap().unwrap();
    assert_eq!(cached_after_failure.available_count, Some(3));

    service.refresh_now(RefreshReason::Manual).unwrap();
    let recovered = wait_for_service_state(&service, 2, false).await;
    assert_eq!(events.wait_for(2, false).await, recovered);
    let cached_recovery = home.cache.load().await.unwrap().unwrap();
    assert_eq!(cached_recovery.available_count, Some(2));
    assert_eq!(server.request_count(), 3);
    assert_eq!(
        events.states.lock().unwrap().as_slice(),
        &[live, stale, recovered]
    );

    service.shutdown().await.unwrap();
    assert!(service.refresh_now(RefreshReason::Manual).is_err());
}

#[tokio::test]
async fn client_transport_handles_live_zero_unsafe_responses_and_recovery() {
    let home = IsolatedCodexHome::new();

    let (live_server, live_result) = fetch_scenario(&home, "live").await;
    let live = live_result.expect("live fixture returns a state");
    assert_eq!(live.available_count, Some(3));
    assert!(!live.stale);
    assert_eq!(live_server.request_count(), 1);
    drop(live_server);

    let (zero_server, zero_result) = fetch_scenario(&home, "zero").await;
    let zero = zero_result.expect("zero is a valid explicit count");
    assert_eq!(zero.available_count, Some(0));
    assert!(!zero.stale);
    assert_eq!(zero_server.request_count(), 1);
    drop(zero_server);

    for scenario in ["disconnect", "malformed"] {
        let (server, result) = fetch_scenario(&home, scenario).await;
        let error = result.expect_err("unsafe response must not produce a state");
        assert_safe_error(&error);
        assert_eq!(server.request_count(), 1);
    }

    let (recovery_server, recovery_result) = fetch_scenario(&home, "recovery").await;
    let recovered = recovery_result.expect("recovery returns a fresh state");
    assert_eq!(recovered.available_count, Some(2));
    assert!(!recovered.stale);
    assert_eq!(recovery_server.request_count(), 1);
}

#[tokio::test]
async fn oversized_valid_response_is_rejected_at_the_body_limit() {
    let home = IsolatedCodexHome::new();
    let (server, result) = fetch_scenario(&home, "oversized").await;
    let error = result.expect_err("oversized valid response must not produce a state");
    assert_safe_error(&error);
    assert!(matches!(
        error,
        BackendError::RpcError(category) if category == "reset_credit_body_limit"
    ));
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn unauthorized_is_unavailable_and_redirect_is_never_followed() {
    let home = IsolatedCodexHome::new();

    let (unauthorized_server, unauthorized_result) = fetch_scenario(&home, "unauthorized").await;
    let unauthorized = unauthorized_result.expect_err("unauthorized response has no state");
    assert_safe_error(&unauthorized);
    assert!(home.cache.load().await.unwrap().is_none());
    assert_eq!(unauthorized_server.request_count(), 1);

    let (redirect_server, redirect_result) = fetch_scenario(&home, "redirect").await;
    let redirect = redirect_result.expect_err("redirect must be rejected");
    assert_safe_error(&redirect);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(redirect_server.request_count(), 1);
}

#[tokio::test]
async fn fake_server_records_header_names_only_and_source_has_no_mutating_route() {
    let home = IsolatedCodexHome::new();
    let (server, result) = fetch_scenario(&home, "live").await;
    assert_eq!(result.unwrap().available_count, Some(3));
    let names = server.observed_header_names();
    for required in [
        "authorization",
        "chatgpt-account-id",
        "openai-beta",
        "originator",
    ] {
        assert!(names.iter().any(|name| name == required));
    }
    let record = fs::read_to_string(server.state_dir.join("header-names.json")).unwrap();
    assert!(!record.contains(SYNTHETIC_TOKEN));
    assert!(!record.contains(SYNTHETIC_ACCOUNT));
    assert!(!record.contains("Bearer"));

    let forbidden_route = format!("/{}", "consume");
    for directory in [
        workspace_root().join("src"),
        workspace_root().join("src-tauri/src"),
    ] {
        assert_no_forbidden_route(&directory, &forbidden_route);
    }
}

fn assert_no_forbidden_route(directory: &Path, forbidden_route: &str) {
    for entry in fs::read_dir(directory).expect("scan production source directory") {
        let path = entry.expect("read production source entry").path();
        if path.is_dir() {
            assert_no_forbidden_route(&path, forbidden_route);
            continue;
        }
        if matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("rs" | "ts" | "tsx")
        ) {
            let source = fs::read_to_string(&path).expect("read UTF-8 production source");
            assert!(
                !source.contains(forbidden_route),
                "production source contains a forbidden mutating route: {}",
                path.display()
            );
        }
    }
}

#[test]
fn acceptance_assets_freeze_weekly_visual_and_coexistence_contract() {
    let root = workspace_root();
    let capture = fs::read_to_string(root.join("scripts/acceptance/capture-widget.ps1"))
        .expect("capture script exists");
    let verify = fs::read_to_string(root.join("scripts/acceptance/verify-window.ps1"))
        .expect("window verification script exists");
    let matrix = fs::read_to_string(root.join("docs/acceptance/gpt-orbit-windows-matrix.md"))
        .expect("acceptance matrix exists");
    let checklist =
        fs::read_to_string(root.join("docs/acceptance/screenshots/capture-checklist.txt"))
            .expect("capture checklist exists");

    let normalized_capture = capture.replace("\r\n", "\n");
    let generated_weekly_checklist = normalized_capture
        .split_once("$notes = if ($Variant -eq \"Weekly\") { @\"\n")
        .expect("weekly checklist template starts")
        .1
        .split_once("\n\"@\n} else")
        .expect("weekly checklist template ends")
        .0;
    assert_eq!(
        generated_weekly_checklist,
        checklist
            .replace("\r\n", "\n")
            .trim_start_matches('\u{feff}')
            .trim_end(),
        "capture script must generate the checked-in Weekly checklist exactly"
    );

    for required in [
        "weekly-collapsed.png",
        "weekly-installed-expanded.png",
        "installed expanded capture remains NOT RUN",
        "no installed expanded image is currently retained",
        "MainWindowHandle",
        "CopyFromScreen",
        "LOCALAPPDATA",
        "expectedWidth",
        "104 x 86",
        "153 x 68",
        "five-hour content must be absent",
        "badge remains fully visible on the right",
    ] {
        assert!(
            capture.contains(required),
            "capture contract missing {required}"
        );
    }
    for forbidden in ["SendInput", "SendKeys", "weekly-expanded.png"] {
        assert!(
            !capture.contains(forbidden),
            "capture script must remain passive and window-scoped: {forbidden}"
        );
    }
    for required in [
        "Variant",
        "weekly-no-flip",
        "weekly-badge-right-visible",
        "weekly-collapsed-visible-size",
        "weekly-expanded-visible-size",
    ] {
        assert!(
            verify.contains(required),
            "window contract missing {required}"
        );
    }
    for required in [
        "Data privacy",
        "Stale fallback",
        "Two installer identities",
        "Simultaneous process",
        "Independent placement",
        "Gpt Orbit_0.1.0_x64-setup.exe",
        "Gpt.Orbit.Weekly_0.1.0_x64-setup.exe",
    ] {
        assert!(
            matrix.contains(required),
            "acceptance matrix missing {required}"
        );
    }
}

#[test]
fn cargo_default_run_keeps_the_test_fixture_out_of_release_bundles() {
    let manifest = fs::read_to_string(workspace_root().join("src-tauri/Cargo.toml"))
        .expect("Cargo manifest exists");
    let package = manifest
        .split_once("[package]")
        .expect("Cargo package section")
        .1
        .split("\n[")
        .next()
        .expect("Cargo package body");

    assert!(workspace_root()
        .join("src-tauri/src/bin/mock_app_server.rs")
        .is_file());
    assert!(
        package
            .lines()
            .any(|line| line.trim() == "default-run = \"codex-orbit\""),
        "multi-bin release must select the Tauri application"
    );
}
