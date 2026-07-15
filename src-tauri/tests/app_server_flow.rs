use codex_orbit_lib::backend::{
    run_headless_supervisor, spawn_app_server, AccountSession, AccountState, RateLimitCache,
    RateLimitRepository, RateLimitSource,
};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};

const TEST_TIMEOUT: Duration = Duration::from_secs(12);
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);
#[cfg(windows)]
static WINDOWS_FIXTURE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct ScenarioFixture {
    directory: PathBuf,
    executable: PathBuf,
}

impl ScenarioFixture {
    fn new(scenario: &str) -> Self {
        let unique = format!(
            "{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock precedes epoch")
                .as_nanos(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        );
        let directory = std::env::temp_dir().join(format!("codex-orbit-flow-{unique}"));
        fs::create_dir_all(&directory).expect("create isolated fixture directory");
        let built_binary = PathBuf::from(env!("CARGO_BIN_EXE_mock_app_server"));
        let binary = directory.join(if cfg!(windows) {
            "mock_app_server.exe"
        } else {
            "mock_app_server"
        });
        fs::copy(&built_binary, &binary).expect("copy fixture into isolated ASCII path");
        fs::write(directory.join("scenario"), scenario).expect("write isolated scenario config");
        let executable = binary;
        Self {
            directory,
            executable,
        }
    }

    fn metric(&self, name: &str) -> u64 {
        fs::read_to_string(self.directory.join(name))
            .unwrap_or_else(|error| panic!("read fixture metric {name}: {error}"))
            .trim()
            .parse()
            .unwrap_or_else(|error| panic!("parse fixture metric {name}: {error}"))
    }
}

impl Drop for ScenarioFixture {
    fn drop(&mut self) {
        if fs::remove_dir_all(&self.directory).is_ok() || !self.directory.exists() {
            return;
        }
        let directory = self.directory.clone();
        std::thread::spawn(move || {
            for _ in 0..120 {
                if fs::remove_dir_all(&directory).is_ok() || !directory.exists() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            let _ = fs::remove_dir_all(directory);
        });
    }
}

async fn read_json_line(child: &mut Child) -> Value {
    let stdout = child.stdout.as_mut().expect("fixture stdout");
    let mut line = Vec::new();
    loop {
        let byte = stdout.read_u8().await.expect("read fixture JSONL");
        if byte == b'\n' {
            break;
        }
        line.push(byte);
    }
    serde_json::from_slice(&line).expect("fixture emitted valid JSON")
}

async fn write_json_line(child: &mut Child, value: &Value) {
    let stdin = child.stdin.as_mut().expect("fixture stdin");
    stdin
        .write_all(serde_json::to_string(value).unwrap().as_bytes())
        .await
        .expect("write fixture JSONL");
    stdin.write_all(b"\n").await.expect("terminate JSONL");
    stdin.flush().await.expect("flush fixture JSONL");
}

#[tokio::test]
async fn handshake_rejects_account_requests_until_initialized_notification() {
    #[cfg(windows)]
    let _fixture_guard = WINDOWS_FIXTURE_LOCK.lock().await;
    tokio::time::timeout(TEST_TIMEOUT, async {
        let fixture = ScenarioFixture::new("logged_in");
        let mut child = Command::new(&fixture.executable)
            .args(["app-server", "--listen", "stdio://"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .expect("spawn fixture");

        write_json_line(
            &mut child,
            &json!({"id": 1, "method": "account/read", "params": {"refreshToken": false}}),
        )
        .await;
        assert_eq!(
            read_json_line(&mut child).await,
            json!({"id": 1, "error": {"code": -32002, "message": "Not initialized"}})
        );

        write_json_line(
            &mut child,
            &json!({"id": 2, "method": "initialize", "params": {"clientInfo": {"name": "test", "title": "Test", "version": "0"}}}),
        )
        .await;
        assert_eq!(read_json_line(&mut child).await["id"], 2);

        write_json_line(
            &mut child,
            &json!({"id": 3, "method": "initialize", "params": {}}),
        )
        .await;
        assert_eq!(
            read_json_line(&mut child).await,
            json!({"id": 3, "error": {"code": -32600, "message": "Already initialized"}})
        );

        write_json_line(
            &mut child,
            &json!({"id": 4, "method": "account/read", "params": {"refreshToken": false}}),
        )
        .await;
        assert_eq!(
            read_json_line(&mut child).await,
            json!({"id": 4, "error": {"code": -32002, "message": "Not initialized"}})
        );

        write_json_line(
            &mut child,
            &json!({"method": "initialized", "params": {}}),
        )
        .await;
        write_json_line(
            &mut child,
            &json!({"id": 5, "method": "initialize", "params": {}}),
        )
        .await;
        assert_eq!(
            read_json_line(&mut child).await,
            json!({"id": 5, "error": {"code": -32600, "message": "Already initialized"}})
        );

        write_json_line(
            &mut child,
            &json!({"id": 6, "method": "account/read", "params": {"refreshToken": false}}),
        )
        .await;
        assert_eq!(read_json_line(&mut child).await["result"]["account"]["type"], "chatgpt");

        child.kill().await.expect("stop fixture");
        child.wait().await.expect("reap fixture");
    })
    .await
    .expect("handshake test timed out");
}

#[tokio::test]
async fn logged_in_process_returns_both_quota_windows() {
    #[cfg(windows)]
    let _fixture_guard = WINDOWS_FIXTURE_LOCK.lock().await;
    tokio::time::timeout(TEST_TIMEOUT, async {
        let fixture = ScenarioFixture::new("logged_in");
        let connection = spawn_app_server(&fixture.executable).await.unwrap();
        let account = AccountSession::new(connection.rpc.clone());
        assert_eq!(
            account.read().await.unwrap(),
            AccountState::Ready {
                plan_type: Some("plus".to_string())
            }
        );

        let repository =
            RateLimitRepository::new(connection.rpc.clone(), Arc::new(|| 1_700_000_000));
        let state = repository.refresh().await.unwrap();
        assert_eq!(state.five_hour.unwrap().used_percent, 20.0);
        assert_eq!(state.weekly.unwrap().used_percent, 40.0);
        connection.shutdown().await;
        #[cfg(windows)]
        fs::remove_file(&fixture.executable)
            .expect("shutdown returned before Windows released the fixture executable");
    })
    .await
    .expect("logged-in test timed out");
}

#[tokio::test]
async fn browser_login_completion_rereads_account() {
    #[cfg(windows)]
    let _fixture_guard = WINDOWS_FIXTURE_LOCK.lock().await;
    tokio::time::timeout(TEST_TIMEOUT, async {
        let fixture = ScenarioFixture::new("login_required");
        let connection = spawn_app_server(&fixture.executable).await.unwrap();
        let account = AccountSession::new(connection.rpc.clone());
        let mut notifications = connection.rpc.subscribe();
        assert_eq!(account.read().await.unwrap(), AccountState::LoginRequired);
        let pending = account.begin_browser_login().await.unwrap();
        assert!(matches!(pending, AccountState::LoginPending { .. }));

        let completion = notifications.recv().await.unwrap();
        assert_eq!(completion.method, "account/login/completed");
        assert_eq!(
            account.handle_notification(&completion).await.unwrap(),
            Some(AccountState::Ready {
                plan_type: Some("plus".to_string())
            })
        );
        connection.shutdown().await;
    })
    .await
    .expect("browser-login test timed out");
}

#[tokio::test]
async fn sparse_update_retains_the_unmentioned_weekly_window() {
    #[cfg(windows)]
    let _fixture_guard = WINDOWS_FIXTURE_LOCK.lock().await;
    tokio::time::timeout(TEST_TIMEOUT, async {
        let fixture = ScenarioFixture::new("sparse_update");
        let connection = spawn_app_server(&fixture.executable).await.unwrap();
        let mut notifications = connection.rpc.subscribe();
        let repository =
            RateLimitRepository::new(connection.rpc.clone(), Arc::new(|| 1_700_000_000));
        let baseline = repository.refresh().await.unwrap();
        assert_eq!(baseline.weekly.as_ref().unwrap().used_percent, 40.0);

        let update = notifications.recv().await.unwrap();
        let updated = repository
            .apply_notification(&update)
            .await
            .unwrap()
            .expect("rate-limit update");
        assert_eq!(updated.source, RateLimitSource::Updated);
        assert_eq!(updated.five_hour.unwrap().used_percent, 55.0);
        assert_eq!(updated.weekly.unwrap().used_percent, 40.0);
        connection.shutdown().await;
    })
    .await
    .expect("sparse-update test timed out");
}

#[tokio::test]
async fn malformed_stdout_line_does_not_poison_following_jsonl() {
    #[cfg(windows)]
    let _fixture_guard = WINDOWS_FIXTURE_LOCK.lock().await;
    tokio::time::timeout(TEST_TIMEOUT, async {
        let fixture = ScenarioFixture::new("bad_json");
        let connection = spawn_app_server(&fixture.executable).await.unwrap();
        let account = AccountSession::new(connection.rpc.clone());
        assert!(matches!(
            account.read().await.unwrap(),
            AccountState::Ready { .. }
        ));
        let repository =
            RateLimitRepository::new(connection.rpc.clone(), Arc::new(|| 1_700_000_000));
        assert!(repository.refresh().await.unwrap().weekly.is_some());
        connection.shutdown().await;
    })
    .await
    .expect("malformed-JSON recovery test timed out");
}

#[tokio::test]
async fn copied_fixture_sidecar_cannot_be_redirected_by_inherited_environment() {
    #[cfg(windows)]
    let _fixture_guard = WINDOWS_FIXTURE_LOCK.lock().await;
    tokio::time::timeout(TEST_TIMEOUT, async {
        let fixture = ScenarioFixture::new("logged_in");
        let redirected = fixture.directory.join("redirected");
        fs::create_dir_all(&redirected).unwrap();
        let mut child = Command::new(&fixture.executable)
            .args(["app-server", "--listen", "stdio://"])
            .env("MOCK_APP_SERVER_SCENARIO", "login_required")
            .env("MOCK_APP_SERVER_DIR", &redirected)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .expect("spawn sidecar fixture");

        write_json_line(
            &mut child,
            &json!({"id": 1, "method": "initialize", "params": {}}),
        )
        .await;
        let _ = read_json_line(&mut child).await;
        write_json_line(&mut child, &json!({"method": "initialized", "params": {}})).await;
        write_json_line(
            &mut child,
            &json!({"id": 2, "method": "account/read", "params": {"refreshToken": false}}),
        )
        .await;
        assert_eq!(
            read_json_line(&mut child).await["result"]["account"]["type"],
            "chatgpt"
        );
        child.kill().await.unwrap();
        child.wait().await.unwrap();
        assert_eq!(fixture.metric("spawn_count"), 1);
        assert!(!redirected.join("spawn_count").exists());
    })
    .await
    .expect("sidecar precedence test timed out");
}

#[tokio::test]
async fn direct_fixture_needs_only_the_documented_scenario_environment() {
    #[cfg(windows)]
    let _fixture_guard = WINDOWS_FIXTURE_LOCK.lock().await;
    tokio::time::timeout(TEST_TIMEOUT, async {
        let fixture = ScenarioFixture::new("logged_in");
        fs::remove_file(fixture.directory.join("scenario")).unwrap();
        let mut child = Command::new(&fixture.executable)
            .args(["app-server", "--listen", "stdio://"])
            .env("MOCK_APP_SERVER_SCENARIO", "logged_in")
            .env_remove("MOCK_APP_SERVER_DIR")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .expect("spawn direct fixture");

        write_json_line(
            &mut child,
            &json!({"id": 1, "method": "initialize", "params": {}}),
        )
        .await;
        let _ = read_json_line(&mut child).await;
        write_json_line(&mut child, &json!({"method": "initialized", "params": {}})).await;
        write_json_line(
            &mut child,
            &json!({"id": 2, "method": "account/read", "params": {"refreshToken": false}}),
        )
        .await;
        assert_eq!(
            read_json_line(&mut child).await["result"]["account"]["type"],
            "chatgpt"
        );
        drop(child.stdin.take());
        assert!(child.wait().await.unwrap().success());
        assert_eq!(fixture.metric("spawn_count"), 1);
        assert!(!fixture.directory.join("active_child").exists());
    })
    .await
    .expect("direct fixture interface test timed out");
}

#[tokio::test]
async fn invalid_invocation_exits_without_lifecycle_markers() {
    #[cfg(windows)]
    let _fixture_guard = WINDOWS_FIXTURE_LOCK.lock().await;
    tokio::time::timeout(TEST_TIMEOUT, async {
        let fixture = ScenarioFixture::new("logged_in");
        fs::remove_file(fixture.directory.join("scenario")).unwrap();
        let status = Command::new(&fixture.executable)
            .arg("wrong-command")
            .env_remove("MOCK_APP_SERVER_SCENARIO")
            .env_remove("MOCK_APP_SERVER_DIR")
            .kill_on_drop(true)
            .status()
            .await
            .expect("run invalid fixture invocation");
        assert_eq!(status.code(), Some(2));
        assert!(!fixture.directory.join("active_child").exists());
        assert!(!fixture.directory.join("spawn_count").exists());
    })
    .await
    .expect("invalid invocation cleanup test timed out");
}

#[tokio::test]
async fn drop_cleanup_allows_async_child_reaping_to_progress() {
    #[cfg(windows)]
    let _fixture_guard = WINDOWS_FIXTURE_LOCK.lock().await;
    tokio::time::timeout(TEST_TIMEOUT, async {
        let fixture = ScenarioFixture::new("logged_in");
        let directory = fixture.directory.clone();
        let connection = spawn_app_server(&fixture.executable).await.unwrap();
        drop(connection);
        drop(fixture);

        tokio::time::timeout(Duration::from_secs(4), async {
            while directory.exists() {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("cleanup blocked Tokio child reaping");
    })
    .await
    .expect("drop cleanup regression timed out");
}

#[tokio::test]
async fn exit_once_can_restart_without_overlapping_children() {
    #[cfg(windows)]
    let _fixture_guard = WINDOWS_FIXTURE_LOCK.lock().await;
    tokio::time::timeout(TEST_TIMEOUT, async {
        let fixture = ScenarioFixture::new("exit_once");
        run_headless_supervisor(
            fixture.executable.clone(),
            RateLimitCache::new(fixture.directory.join("rate-limits.json")),
        )
        .await
        .expect("supervisor did not recover after fixture exit");

        assert_eq!(fixture.metric("spawn_count"), 2);
        assert_eq!(fixture.metric("max_simultaneous_children"), 1);
    })
    .await
    .expect("restart test timed out");
}
