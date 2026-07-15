use codex_orbit_lib::backend::{
    run_headless_supervisor, spawn_app_server, AccountSession, AccountState, RateLimitCache,
    RateLimitRepository, RateLimitSource,
};
use codex_orbit_lib::desktop::app_variant::{AppVariant, STANDARD_CANVAS, WEEKLY_CANVAS};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static NEXT_SCENARIO: AtomicU64 = AtomicU64::new(1);

struct FakeServerFixture {
    directory: PathBuf,
    executable: PathBuf,
}

impl FakeServerFixture {
    fn new(scenario: &str) -> Self {
        let root = workspace_root();
        let unique = format!(
            "{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock before epoch")
                .as_nanos(),
            NEXT_SCENARIO.fetch_add(1, Ordering::Relaxed)
        );
        let directory = std::env::temp_dir().join(format!("gpt-orbit-fake-{unique}"));
        fs::create_dir_all(&directory).expect("create fake state directory");
        let executable = directory.join("fake-app-server.cmd");
        let script = directory.join("server.mjs");
        fs::copy(root.join("tests/fake-app-server/server.mjs"), &script)
            .expect("copy fake app-server into ASCII temp directory");
        fs::write(
            &executable,
            format!(
                "@echo off\r\nset GPT_ORBIT_FAKE_SCENARIO={scenario}\r\nset GPT_ORBIT_FAKE_STATE_DIR={}\r\nnode \"{}\" %*\r\n",
                directory.display(),
                script.display()
            ),
        )
        .expect("write fake app-server wrapper");
        Self {
            directory,
            executable,
        }
    }

    fn metric(&self, name: &str) -> u64 {
        fs::read_to_string(self.directory.join(name))
            .unwrap_or_else(|error| panic!("read metric {name}: {error}"))
            .trim()
            .parse()
            .unwrap_or_else(|error| panic!("parse metric {name}: {error}"))
    }
}

impl Drop for FakeServerFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

#[test]
fn fake_app_server_declares_every_release_scenario_without_secret_inputs() {
    let root = workspace_root();
    let script = fs::read_to_string(root.join("tests/fake-app-server/server.mjs"))
        .expect("fake app-server script exists");

    for scenario in [
        "live",
        "sparse-weekly",
        "weekly-missing",
        "login-required",
        "disconnect-once",
        "malformed-then-valid",
    ] {
        assert!(
            script.contains(scenario),
            "missing fake scenario {scenario}"
        );
    }
    assert!(!script.contains("OPENAI_API_KEY"));
    assert!(!script.contains("CODEX_HOME"));
    assert!(!script.contains("chatgpt.com/backend-api"));
}

#[test]
fn tauri_bundle_targets_unsigned_current_user_nsis() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let config: Value = serde_json::from_str(
        &fs::read_to_string(manifest_dir.join("tauri.conf.json")).expect("tauri config exists"),
    )
    .expect("valid tauri config JSON");

    assert_eq!(config["bundle"]["active"], true);
    assert_eq!(config["mainBinaryName"], "codex-orbit");
    assert_eq!(config["bundle"]["targets"], serde_json::json!(["nsis"]));
    assert_eq!(
        config["bundle"]["windows"]["nsis"]["installMode"],
        "currentUser"
    );
    assert!(config["bundle"].get("publisher").is_none());
}

#[test]
fn weekly_build_has_isolated_identity_and_full_window_security() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let standard: Value = serde_json::from_str(
        &fs::read_to_string(manifest_dir.join("tauri.conf.json"))
            .expect("standard tauri config exists"),
    )
    .expect("valid standard tauri config JSON");
    let weekly: Value = serde_json::from_str(
        &fs::read_to_string(manifest_dir.join("tauri.weekly.conf.json"))
            .expect("weekly tauri config exists"),
    )
    .expect("valid weekly tauri config JSON");

    for field in ["productName", "identifier", "mainBinaryName"] {
        assert_ne!(
            standard[field], weekly[field],
            "weekly {field} must be isolated"
        );
    }
    assert_eq!(weekly["productName"], "Gpt Orbit Weekly");
    assert_eq!(weekly["mainBinaryName"], "gpt-orbit-weekly");
    assert_eq!(weekly["identifier"], "com.codex-orbit.weekly");

    let window = &weekly["app"]["windows"][0];
    assert_eq!(
        window["title"], "",
        "transparent weekly window must not expose native title text"
    );
    for flag in ["transparent", "alwaysOnTop", "skipTaskbar"] {
        assert_eq!(window[flag], true, "weekly window must preserve {flag}");
    }
    for flag in ["decorations", "resizable", "shadow", "visible"] {
        assert_eq!(window[flag], false, "weekly window must preserve {flag}");
    }
    assert_eq!(window["width"], 104);
    assert_eq!(window["minWidth"], 104);
    assert_eq!(window["height"], 86);
    assert_eq!(window["maxHeight"], 86);
    assert_eq!(window["minHeight"], 68);
    assert_eq!(window["maxWidth"], 153);
}

#[test]
fn app_variant_and_widget_canvas_contract_is_frozen() {
    assert_eq!(
        AppVariant::from_identifier("com.codex-orbit.weekly"),
        AppVariant::Weekly
    );
    assert_eq!(
        AppVariant::from_identifier("com.codex-orbit.app"),
        AppVariant::Standard
    );
    assert_eq!(
        AppVariant::from_identifier("unexpected"),
        AppVariant::Standard
    );
    assert_eq!(STANDARD_CANVAS.collapsed_width, 172.0);
    assert_eq!(STANDARD_CANVAS.collapsed_height, 172.0);
    assert_eq!(STANDARD_CANVAS.expanded_width, 269.0);
    assert_eq!(STANDARD_CANVAS.expanded_height, 136.0);
    assert_eq!(WEEKLY_CANVAS.collapsed_width, 104.0);
    assert_eq!(WEEKLY_CANVAS.collapsed_height, 86.0);
    assert_eq!(WEEKLY_CANVAS.expanded_width, 153.0);
    assert_eq!(WEEKLY_CANVAS.expanded_height, 68.0);
}

#[test]
fn acceptance_scripts_and_matrix_are_present() {
    let root = workspace_root();
    for path in [
        "scripts/acceptance/capture-widget.ps1",
        "scripts/acceptance/verify-window.ps1",
        "docs/acceptance/gpt-orbit-windows-matrix.md",
    ] {
        assert!(root.join(path).is_file(), "missing {path}");
    }
}

#[tokio::test]
async fn fake_live_supervisor_retains_state_and_reaps_one_child() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let fixture = FakeServerFixture::new("live");
        run_headless_supervisor(
            fixture.executable.clone(),
            RateLimitCache::new(fixture.directory.join("rate-limits.json")),
        )
        .await
        .expect("live fake supervisor should publish retained state");

        assert_eq!(fixture.metric("spawn-count"), 1);
        assert_eq!(fixture.metric("max-simultaneous-children"), 1);
    })
    .await
    .expect("live fake supervisor test timed out");
}

#[tokio::test]
async fn fake_sparse_weekly_merge_retains_unmentioned_weekly_window() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let fixture = FakeServerFixture::new("sparse-weekly");
        let connection = spawn_app_server(&fixture.executable).await.unwrap();
        let repository =
            RateLimitRepository::new(connection.rpc.clone(), Arc::new(|| 1_800_000_000));
        let mut notifications = connection.rpc.subscribe();
        let baseline = repository.refresh().await.unwrap();
        assert_eq!(baseline.weekly.as_ref().unwrap().remaining_percent, 58.0);

        let update = notifications.recv().await.unwrap();
        let updated = repository
            .apply_notification(&update)
            .await
            .unwrap()
            .expect("sparse update");
        assert_eq!(updated.source, RateLimitSource::Updated);
        assert_eq!(updated.five_hour.unwrap().remaining_percent, 45.0);
        assert_eq!(updated.weekly.unwrap().remaining_percent, 58.0);
        connection.shutdown().await;
    })
    .await
    .expect("sparse weekly fake test timed out");
}

#[tokio::test]
async fn fake_weekly_missing_contract_has_no_weekly_window() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let fixture = FakeServerFixture::new("weekly-missing");
        let connection = spawn_app_server(&fixture.executable).await.unwrap();
        let repository =
            RateLimitRepository::new(connection.rpc.clone(), Arc::new(|| 1_800_000_000));
        let state = repository.refresh().await.unwrap();
        assert!(state.five_hour.is_some());
        assert!(state.weekly.is_none());
        connection.shutdown().await;
    })
    .await
    .expect("weekly missing fake test timed out");
}

#[tokio::test]
async fn fake_login_required_emits_safe_https_login_url_then_recovers() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let fixture = FakeServerFixture::new("login-required");
        let connection = spawn_app_server(&fixture.executable).await.unwrap();
        let account = AccountSession::new(connection.rpc.clone());
        let mut notifications = connection.rpc.subscribe();
        assert_eq!(account.read().await.unwrap(), AccountState::LoginRequired);

        let pending = account.begin_browser_login().await.unwrap();
        let AccountState::LoginPending { auth_url, .. } = pending else {
            panic!("expected pending login");
        };
        assert!(auth_url.starts_with("https://example.invalid/"));
        let completion = notifications.recv().await.unwrap();
        assert_eq!(completion.method, "account/login/completed");
        assert!(matches!(
            account.handle_notification(&completion).await.unwrap(),
            Some(AccountState::Ready { .. })
        ));
        connection.shutdown().await;
    })
    .await
    .expect("login fake test timed out");
}

#[tokio::test]
async fn fake_disconnect_once_restarts_without_overlapping_children() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let fixture = FakeServerFixture::new("disconnect-once");
        run_headless_supervisor(
            fixture.executable.clone(),
            RateLimitCache::new(fixture.directory.join("rate-limits.json")),
        )
        .await
        .expect("disconnect-once fake should reconnect");

        assert_eq!(fixture.metric("spawn-count"), 2);
        assert_eq!(fixture.metric("max-simultaneous-children"), 1);
    })
    .await
    .expect("disconnect fake test timed out");
}

#[tokio::test]
async fn fake_malformed_then_valid_does_not_poison_protocol() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let fixture = FakeServerFixture::new("malformed-then-valid");
        let connection = spawn_app_server(&fixture.executable).await.unwrap();
        let repository =
            RateLimitRepository::new(connection.rpc.clone(), Arc::new(|| 1_800_000_000));
        let state = repository.refresh().await.unwrap();
        assert!(state.five_hour.is_some());
        assert!(state.weekly.is_some());
        connection.shutdown().await;
    })
    .await
    .expect("malformed fake test timed out");
}
