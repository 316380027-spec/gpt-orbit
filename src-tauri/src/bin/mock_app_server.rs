use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

struct ActiveChild {
    marker: PathBuf,
    owned: bool,
}

impl Drop for ActiveChild {
    fn drop(&mut self) {
        if self.owned {
            let _ = fs::remove_file(&self.marker);
        }
    }
}

fn main() -> ExitCode {
    let executable = std::env::current_exe().expect("resolve fixture executable");
    let executable_directory = executable.parent().expect("fixture executable parent");
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments != ["app-server", "--listen", "stdio://"] {
        return ExitCode::from(2);
    }

    let sidecar = executable_directory.join("scenario");
    let (directory, scenario) = if sidecar.is_file() {
        (
            executable_directory.to_path_buf(),
            fs::read_to_string(sidecar).unwrap_or_default(),
        )
    } else {
        (
            std::env::var_os("MOCK_APP_SERVER_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| executable_directory.to_path_buf()),
            std::env::var("MOCK_APP_SERVER_SCENARIO").unwrap_or_default(),
        )
    };
    if !matches!(
        scenario.as_str(),
        "logged_in" | "login_required" | "sparse_update" | "bad_json" | "exit_once"
    ) {
        return ExitCode::from(3);
    }

    fs::create_dir_all(&directory).expect("create scenario directory");
    let _active = record_child_start(&directory);

    if scenario == "exit_once" {
        let first_exit = directory.join("exit_once_marker");
        if OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(first_exit)
            .is_ok()
        {
            return ExitCode::SUCCESS;
        }
    }

    serve(&scenario);
    ExitCode::SUCCESS
}

fn record_child_start(directory: &Path) -> ActiveChild {
    increment_metric(&directory.join("spawn_count"));
    let marker = directory.join("active_child");
    let owned = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker)
        .is_ok();
    let maximum = directory.join("max_simultaneous_children");
    if !owned {
        fs::write(&maximum, "2").expect("record overlapping children");
    } else if !maximum.exists() {
        fs::write(&maximum, "1").expect("record first child");
    }
    ActiveChild { marker, owned }
}

fn increment_metric(path: &Path) {
    let current = fs::read_to_string(path)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(0);
    fs::write(path, (current + 1).to_string()).expect("update fixture metric");
}

fn serve(scenario: &str) {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum InitializePhase {
        PreInitialize,
        AwaitingInitialized,
        Initialized,
    }

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut output = stdout.lock();
    let mut phase = InitializePhase::PreInitialize;
    let mut login_completed = scenario != "login_required";
    let mut sent_bad_json = false;
    let mut sent_sparse_update = false;

    for line in stdin.lock().lines() {
        let Ok(line) = line else {
            return;
        };
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let method = message.get("method").and_then(Value::as_str);
        let id = message.get("id").cloned();

        if method == Some("initialize") {
            if phase != InitializePhase::PreInitialize {
                if let Some(id) = id {
                    respond(
                        &mut output,
                        json!({"id": id, "error": {"code": -32600, "message": "Already initialized"}}),
                    );
                }
                continue;
            }
            if scenario == "bad_json" && !sent_bad_json {
                writeln!(output, "{{not-json").expect("write malformed fixture line");
                output.flush().expect("flush malformed fixture line");
                sent_bad_json = true;
            }
            if let Some(id) = id {
                respond(
                    &mut output,
                    json!({
                        "id": id,
                        "result": {
                            "userAgent": "mock-app-server",
                            "codexHome": "isolated-mock-home",
                            "platformFamily": "mock",
                            "platformOs": "mock"
                        }
                    }),
                );
                phase = InitializePhase::AwaitingInitialized;
            }
            continue;
        }

        if method == Some("initialized") && id.is_none() {
            if phase != InitializePhase::AwaitingInitialized {
                return;
            }
            phase = InitializePhase::Initialized;
            continue;
        }

        if let Some(id) = id {
            if phase != InitializePhase::Initialized {
                respond(
                    &mut output,
                    json!({"id": id, "error": {"code": -32002, "message": "Not initialized"}}),
                );
                continue;
            }

            match method {
                Some("account/read") => {
                    let account = if login_completed {
                        json!({
                            "type": "chatgpt",
                            "email": "fixture@example.invalid",
                            "planType": "plus",
                            "futureField": {"ignored": true}
                        })
                    } else {
                        Value::Null
                    };
                    respond(
                        &mut output,
                        json!({"id": id, "result": {"account": account}}),
                    );
                }
                Some("account/login/start") => {
                    login_completed = true;
                    respond(
                        &mut output,
                        json!({
                            "id": id,
                            "result": {
                                "loginId": "fixture-login-1",
                                "authUrl": "https://example.invalid/mock-login"
                            }
                        }),
                    );
                    respond(
                        &mut output,
                        json!({
                            "method": "account/login/completed",
                            "params": {
                                "loginId": "fixture-login-1",
                                "success": true,
                                "error": null
                            }
                        }),
                    );
                }
                Some("account/rateLimits/read") => {
                    respond(
                        &mut output,
                        json!({
                            "id": id,
                            "result": {
                                "rateLimits": {
                                    "primary": {
                                        "usedPercent": 20,
                                        "windowDurationMins": 300,
                                        "resetsAt": 1800000000
                                    },
                                    "secondary": {
                                        "usedPercent": 40,
                                        "windowDurationMins": 10080,
                                        "resetsAt": 1800500000
                                    },
                                    "planType": "plus"
                                },
                                "unknownResponseField": true
                            }
                        }),
                    );
                    if scenario == "sparse_update" && !sent_sparse_update {
                        respond(
                            &mut output,
                            json!({
                                "method": "account/rateLimits/updated",
                                "params": {
                                    "rateLimits": {"primary": {"usedPercent": 55}}
                                }
                            }),
                        );
                        sent_sparse_update = true;
                    }
                }
                _ => respond(
                    &mut output,
                    json!({"id": id, "error": {"code": -32601, "message": "Method not found"}}),
                ),
            }
        }
    }
}

fn respond(output: &mut impl Write, message: Value) {
    serde_json::to_writer(&mut *output, &message).expect("serialize fixture response");
    output.write_all(b"\n").expect("terminate fixture JSONL");
    output.flush().expect("flush fixture JSONL");
}
