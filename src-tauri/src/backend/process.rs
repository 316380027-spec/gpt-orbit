use super::{initialize, BackendError, BackendResult, RpcClient};
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::oneshot;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

struct AppServerShutdown {
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl Drop for AppServerShutdown {
    fn drop(&mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
    }
}

pub struct AppServerConnection {
    _shutdown: AppServerShutdown,
    pub rpc: RpcClient,
    pub exit: oneshot::Receiver<ExitStatus>,
}

pub struct AppServerStartup {
    shutdown: AppServerShutdown,
    rpc: RpcClient,
    exit: oneshot::Receiver<ExitStatus>,
}

impl AppServerConnection {
    pub async fn shutdown(self) {
        let Self {
            _shutdown,
            rpc,
            exit,
        } = self;
        rpc.disconnect().await;
        drop(_shutdown);
        let _ = exit.await;
    }
}

impl AppServerStartup {
    pub async fn initialize(&self) -> BackendResult<()> {
        initialize(&self.rpc, env!("CARGO_PKG_VERSION")).await
    }

    pub fn into_connection(self) -> AppServerConnection {
        AppServerConnection {
            _shutdown: self.shutdown,
            rpc: self.rpc,
            exit: self.exit,
        }
    }

    pub async fn shutdown(self) {
        self.rpc.disconnect().await;
        drop(self.shutdown);
        let _ = self.exit.await;
    }
}

pub fn begin_app_server(executable: &Path) -> BackendResult<AppServerStartup> {
    let mut command = Command::new(executable);
    command
        .args(["app-server", "--listen", "stdio://"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let mut child = command
        .spawn()
        .map_err(|_| BackendError::RpcError("app_server_start".to_string()))?;
    tracing::debug!(category = "app_server_started");

    let stdin = match child.stdin.take() {
        Some(stdin) => stdin,
        None => {
            reclaim_child(child);
            return Err(BackendError::RpcError("app_server_stdin".to_string()));
        }
    };
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            reclaim_child(child);
            return Err(BackendError::RpcError("app_server_stdout".to_string()));
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            reclaim_child(child);
            return Err(BackendError::RpcError("app_server_stderr".to_string()));
        }
    };
    let rpc = RpcClient::new(stdin);

    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();
    let shutdown = AppServerShutdown {
        shutdown_tx: Some(shutdown_tx),
    };
    let (exit_tx, exit) = oneshot::channel();
    let owner_rpc = rpc.clone();
    tokio::spawn(async move {
        let wait_result = tokio::select! {
            result = child.wait() => result,
            _ = &mut shutdown_rx => {
                tracing::debug!(category = "app_server_shutdown");
                let _ = child.kill().await;
                child.wait().await
            }
        };
        owner_rpc.disconnect().await;
        match wait_result {
            Ok(status) => {
                tracing::debug!(category = "app_server_exited");
                let _ = exit_tx.send(status);
            }
            Err(_) => tracing::warn!(category = "app_server_wait_error"),
        }
    });

    let stdout_rpc = rpc.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    if stdout_rpc.accept_line(&line).await.is_err() {
                        tracing::warn!(category = "app_server_protocol_error");
                    }
                }
                Ok(None) => break,
                Err(_) => {
                    tracing::warn!(category = "app_server_stdout_error");
                    break;
                }
            }
        }
        stdout_rpc.disconnect().await;
    });

    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut saw_stderr = false;
        loop {
            match lines.next_line().await {
                Ok(Some(_)) => saw_stderr = true,
                Ok(None) => break,
                Err(_) => {
                    tracing::warn!(category = "app_server_stderr_error");
                    return;
                }
            }
        }
        if saw_stderr {
            tracing::debug!(category = "app_server_stderr_output");
        }
    });

    Ok(AppServerStartup {
        shutdown,
        rpc,
        exit,
    })
}

pub async fn spawn_app_server(executable: &Path) -> BackendResult<AppServerConnection> {
    let startup = begin_app_server(executable)?;
    if let Err(error) = startup.initialize().await {
        startup.shutdown().await;
        return Err(error);
    }
    Ok(startup.into_connection())
}

fn reclaim_child(mut child: tokio::process::Child) {
    tokio::spawn(async move {
        let _ = child.kill().await;
        let _ = child.wait().await;
        tracing::debug!(category = "app_server_reclaimed");
    });
}

#[cfg(test)]
mod tests {
    use super::{begin_app_server, spawn_app_server};
    use crate::backend::supervisor::{wait_or_shutdown, AwaitOutcome};
    use crate::backend::{BackendError, ServiceCommand};
    use serde_json::Value;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command as StdCommand;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(1);

    #[cfg(windows)]
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit_handle: i32, process_id: u32) -> *mut std::ffi::c_void;
        fn GetExitCodeProcess(process: *mut std::ffi::c_void, exit_code: *mut u32) -> i32;
        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    }

    #[cfg(windows)]
    fn fixture_id() -> String {
        format!(
            "{}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed)
        )
    }

    #[cfg(windows)]
    fn compile_lifecycle_fixture() -> (PathBuf, PathBuf) {
        let unique = fixture_id();
        let directory = std::env::temp_dir().join(format!("codex-orbit-owner-{unique}"));
        fs::create_dir_all(&directory).unwrap();
        let source = directory.join("fixture.rs");
        let executable = directory.join("fixture.exe");
        fs::write(
            &source,
            r##"use std::fs;
use std::io::{self, BufRead, Write};
use std::process::{Command, Stdio};

fn main() {
    let executable = std::env::current_exe().unwrap();
    fs::write(executable.with_extension("pid"), std::process::id().to_string()).unwrap();
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    let _initialize = lines.next();
    println!(r#"{{"id":1,"result":{{}}}}"#);
    io::stdout().flush().unwrap();
    let _initialized = lines.next();
    if lines.next().is_some() {
        let _descendant = Command::new("cmd.exe")
            .args(["/d", "/c", "ping -n 6 127.0.0.1 >nul"])
            .stdout(Stdio::inherit())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
    }
}
"##,
        )
        .unwrap();
        let output = StdCommand::new("rustc")
            .args(["--edition=2021", "-o"])
            .arg(&executable)
            .arg(&source)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "fixture compilation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        (executable, directory)
    }

    #[cfg(windows)]
    fn compile_unresponsive_initialize_fixture() -> (PathBuf, PathBuf) {
        let unique = fixture_id();
        let directory = std::env::temp_dir().join(format!("codex-orbit-init-abort-{unique}"));
        fs::create_dir_all(&directory).unwrap();
        let source = directory.join("fixture.rs");
        let executable = directory.join("fixture.exe");
        fs::write(
            &source,
            r#"use std::fs;
use std::io::{self, BufRead};
use std::thread;
use std::time::Duration;

fn main() {
    let executable = std::env::current_exe().unwrap();
    fs::write(executable.with_extension("pid"), std::process::id().to_string()).unwrap();
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    let _initialize = lines.next();
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}
"#,
        )
        .unwrap();
        let output = StdCommand::new("rustc")
            .args(["--edition=2021", "-o"])
            .arg(&executable)
            .arg(&source)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "fixture compilation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        (executable, directory)
    }

    #[cfg(windows)]
    fn compile_unresponsive_request_fixture() -> (PathBuf, PathBuf) {
        let unique = fixture_id();
        let directory = std::env::temp_dir().join(format!("codex-orbit-request-block-{unique}"));
        fs::create_dir_all(&directory).unwrap();
        let source = directory.join("fixture.rs");
        let executable = directory.join("fixture.exe");
        fs::write(
            &source,
            r##"use std::fs;
use std::io::{self, BufRead, Write};
use std::thread;
use std::time::Duration;

fn main() {
    let executable = std::env::current_exe().unwrap();
    fs::write(executable.with_extension("pid"), std::process::id().to_string()).unwrap();
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    let _initialize = lines.next();
    println!(r#"{{"id":1,"result":{{}}}}"#);
    io::stdout().flush().unwrap();
    let _initialized = lines.next();
    let _blocked_request = lines.next();
    loop { thread::sleep(Duration::from_secs(60)); }
}
"##,
        )
        .unwrap();
        let output = StdCommand::new("rustc")
            .args(["--edition=2021", "-o"])
            .arg(&executable)
            .arg(&source)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "fixture compilation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        (executable, directory)
    }

    #[cfg(windows)]
    fn compile_stdout_eof_fixture() -> (PathBuf, PathBuf) {
        let unique = fixture_id();
        let directory = std::env::temp_dir().join(format!("codex-orbit-stdio-eof-{unique}"));
        fs::create_dir_all(&directory).unwrap();
        let source = directory.join("fixture.rs");
        let executable = directory.join("fixture.exe");
        fs::write(
            &source,
            r##"use std::ffi::c_void;
use std::fs;
use std::io::{self, BufRead, Write};
use std::thread;
use std::time::Duration;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetStdHandle(kind: u32) -> *mut c_void;
    fn CloseHandle(handle: *mut c_void) -> i32;
}

fn main() {
    let executable = std::env::current_exe().unwrap();
    fs::write(executable.with_extension("pid"), std::process::id().to_string()).unwrap();
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    let _initialize = lines.next();
    println!(r#"{{"id":1,"result":{{}}}}"#);
    io::stdout().flush().unwrap();
    let _initialized = lines.next();
    unsafe { CloseHandle(GetStdHandle(-11_i32 as u32)); }
    fs::write(executable.with_extension("closed"), b"closed").unwrap();
    loop { thread::sleep(Duration::from_secs(60)); }
}
"##,
        )
        .unwrap();
        let output = StdCommand::new("rustc")
            .args(["--edition=2021", "-o"])
            .arg(&executable)
            .arg(&source)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "fixture compilation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        (executable, directory)
    }

    #[cfg(windows)]
    fn process_is_running(pid: u32) -> bool {
        const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
        const STILL_ACTIVE: u32 = 259;
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return false;
            }
            let mut exit_code = 0;
            let queried = GetExitCodeProcess(process, &mut exit_code);
            let _ = CloseHandle(process);
            queried != 0 && exit_code == STILL_ACTIVE
        }
    }

    #[cfg(windows)]
    async fn wait_for_process_exit(pid: u32, timeout: std::time::Duration) -> bool {
        tokio::time::timeout(timeout, async {
            while process_is_running(pid) {
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        })
        .await
        .is_ok()
    }

    #[cfg(windows)]
    async fn wait_for_fixture_pid(executable: &Path) -> u32 {
        let pid_file = executable.with_extension("pid");
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if let Ok(text) = fs::read_to_string(&pid_file) {
                    if let Ok(pid) = text.trim().parse::<u32>() {
                        return pid;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fixture did not publish a complete PID within 10 seconds")
    }

    #[cfg(windows)]
    fn cleanup_fixture(directory: &Path, pid: u32) {
        if process_is_running(pid) {
            let _ = StdCommand::new("taskkill.exe")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .output();
        }
        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn spawns_stdio_app_server() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let fixture = std::env::temp_dir().join(format!("codex-orbit-app-server-{unique}.cmd"));
        fs::write(
            &fixture,
            r#"@echo off
if not "%1"=="app-server" exit /b 11
if not "%2"=="--listen" exit /b 12
if not "%3"=="stdio://" exit /b 13
set /p initialize=
echo {"id":1,"result":{}}
set /p initialized=
exit /b 0
"#,
        )
        .unwrap();

        let connection = spawn_app_server(&fixture).await.unwrap();
        let status = connection.exit.await.unwrap();
        let _ = fs::remove_file(&fixture);

        assert!(status.success(), "fixture rejected app-server arguments");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn dropping_connection_stops_child_within_timeout() {
        let (fixture, directory) = compile_lifecycle_fixture();
        let connection = spawn_app_server(&fixture).await.unwrap();
        let pid: u32 = fs::read_to_string(fixture.with_extension("pid"))
            .unwrap()
            .parse()
            .unwrap();
        assert!(process_is_running(pid));

        drop(connection);
        let exited = wait_for_process_exit(pid, std::time::Duration::from_secs(1)).await;
        cleanup_fixture(&directory, pid);

        assert!(exited, "dropping the connection left the child running");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn shutdown_waits_until_child_is_reaped() {
        let (fixture, directory) = compile_lifecycle_fixture();
        let connection = spawn_app_server(&fixture).await.unwrap();
        let pid: u32 = fs::read_to_string(fixture.with_extension("pid"))
            .unwrap()
            .parse()
            .unwrap();

        connection.shutdown().await;
        let exited = !process_is_running(pid);
        cleanup_fixture(&directory, pid);

        assert!(exited, "shutdown returned before the child was reaped");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn shutdown_cancels_blocked_rpc_and_reaps_quickly() {
        let (fixture, directory) = compile_unresponsive_request_fixture();
        let connection = spawn_app_server(&fixture).await.unwrap();
        let pid: u32 = fs::read_to_string(fixture.with_extension("pid"))
            .unwrap()
            .parse()
            .unwrap();
        let rpc = connection.rpc.clone();
        let pending =
            tokio::spawn(
                async move { rpc.request::<_, Value>("account/read", None::<Value>).await },
            );

        tokio::time::timeout(std::time::Duration::from_millis(500), connection.shutdown())
            .await
            .expect("shutdown waited for the blocked RPC timeout");
        let pending_result = pending.await.unwrap();
        let exited = !process_is_running(pid);
        cleanup_fixture(&directory, pid);

        assert!(matches!(pending_result, Err(BackendError::RpcDisconnected)));
        assert!(exited, "blocked RPC shutdown returned before child reap");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn stdout_eof_disconnect_is_retained_while_child_is_alive() {
        let (fixture, directory) = compile_stdout_eof_fixture();
        let connection = spawn_app_server(&fixture).await.unwrap();
        let pid: u32 = fs::read_to_string(fixture.with_extension("pid"))
            .unwrap()
            .parse()
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while !fixture.with_extension("closed").exists() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        tokio::time::timeout(
            std::time::Duration::from_millis(500),
            connection.rpc.wait_disconnected(),
        )
        .await
        .expect("stdout EOF did not publish disconnect");
        assert!(
            process_is_running(pid),
            "fixture exited instead of closing stdio"
        );

        connection.shutdown().await;
        let exited = !process_is_running(pid);
        cleanup_fixture(&directory, pid);
        assert!(exited, "stdio-disconnected child was not reaped");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn child_exit_immediately_disconnects_pending_rpc() {
        let (fixture, directory) = compile_lifecycle_fixture();
        let connection = spawn_app_server(&fixture).await.unwrap();
        let pid: u32 = fs::read_to_string(fixture.with_extension("pid"))
            .unwrap()
            .parse()
            .unwrap();
        let rpc = connection.rpc.clone();

        let pending =
            tokio::spawn(
                async move { rpc.request::<_, Value>("fixture/exit", None::<Value>).await },
            );
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), pending).await;
        cleanup_fixture(&directory, pid);

        assert!(matches!(
            result.unwrap().unwrap(),
            Err(BackendError::RpcDisconnected)
        ));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn cancelling_spawn_during_initialize_stops_child() {
        let (fixture, directory) = compile_unresponsive_initialize_fixture();
        let fixture_for_spawn = fixture.clone();
        let spawn = tokio::spawn(async move { spawn_app_server(&fixture_for_spawn).await });
        let pid = wait_for_fixture_pid(&fixture).await;

        spawn.abort();
        assert!(matches!(spawn.await, Err(error) if error.is_cancelled()));
        let exited = wait_for_process_exit(pid, std::time::Duration::from_secs(1)).await;
        cleanup_fixture(&directory, pid);

        assert!(
            exited,
            "cancelling spawn left the initializing child running"
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn shutdown_during_initialize_waits_until_startup_child_is_reaped() {
        let (fixture, directory) = compile_unresponsive_initialize_fixture();
        let startup = begin_app_server(&fixture).unwrap();
        let pid = wait_for_fixture_pid(&fixture).await;
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let waiter = tokio::spawn(async move {
            let mut pending_refresh = false;
            let outcome =
                wait_or_shutdown(&mut rx, &mut pending_refresh, startup.initialize()).await;
            if matches!(outcome, AwaitOutcome::Shutdown) {
                startup.shutdown().await;
            }
        });

        tx.send(ServiceCommand::Shutdown).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
            .await
            .expect("service shutdown returned before startup owner reaped child")
            .unwrap();
        let exited = !process_is_running(pid);
        cleanup_fixture(&directory, pid);

        assert!(
            exited,
            "startup shutdown completed while PID was still alive"
        );
    }
}
