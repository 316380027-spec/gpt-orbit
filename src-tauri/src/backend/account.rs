use super::{BackendError, BackendResult, RpcClient, RpcNotification};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::fmt;
use std::sync::Arc;
use tokio::sync::{watch, Mutex};

#[async_trait]
pub trait RpcTransport: Clone + Send + Sync + 'static {
    async fn request_value(
        &self,
        method: &'static str,
        params: Option<Value>,
    ) -> BackendResult<Value>;
}

#[async_trait]
impl RpcTransport for RpcClient {
    async fn request_value(
        &self,
        method: &'static str,
        params: Option<Value>,
    ) -> BackendResult<Value> {
        self.request(method, params).await
    }
}

#[derive(Clone, PartialEq)]
pub enum AccountState {
    Ready { plan_type: Option<String> },
    LoginRequired,
    LoginPending { login_id: String, auth_url: String },
}

impl fmt::Debug for AccountState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ready { plan_type } => formatter
                .debug_struct("Ready")
                .field("plan_type", plan_type)
                .finish(),
            Self::LoginRequired => formatter.write_str("LoginRequired"),
            Self::LoginPending {
                login_id,
                auth_url: _,
            } => formatter
                .debug_struct("LoginPending")
                .field("login_id", login_id)
                .field("auth_url", &"<redacted>")
                .finish(),
        }
    }
}

#[derive(Clone)]
struct PendingLogin {
    login_id: String,
    auth_url: String,
}

enum LoginStatus {
    Idle,
    Starting {
        outcome: watch::Receiver<Option<BackendResult<PendingLogin>>>,
    },
    Pending(PendingLogin),
}

pub struct AccountSession<T: RpcTransport> {
    rpc: T,
    login_status: Arc<Mutex<LoginStatus>>,
}

impl<T: RpcTransport> fmt::Debug for AccountSession<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AccountSession")
            .finish_non_exhaustive()
    }
}

impl<T: RpcTransport> AccountSession<T> {
    pub fn new(rpc: T) -> Self {
        Self {
            rpc,
            login_status: Arc::new(Mutex::new(LoginStatus::Idle)),
        }
    }

    pub async fn read(&self) -> BackendResult<AccountState> {
        let response = self
            .rpc
            .request_value("account/read", Some(json!({"refreshToken": false})))
            .await?;
        let account = response.get("account");
        if account.and_then(|value| value.get("type")) == Some(&Value::String("chatgpt".into())) {
            let plan_type = account
                .and_then(|value| value.get("planType"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            return Ok(AccountState::Ready { plan_type });
        }

        Ok(AccountState::LoginRequired)
    }

    pub async fn begin_browser_login(&self) -> BackendResult<AccountState> {
        let mut outcome = {
            let mut status = self.login_status.lock().await;
            match &*status {
                LoginStatus::Pending(pending) => return Ok(pending.as_account_state()),
                LoginStatus::Starting { outcome } => outcome.clone(),
                LoginStatus::Idle => {
                    let (outcome_tx, outcome_rx) = watch::channel(None);
                    *status = LoginStatus::Starting {
                        outcome: outcome_rx.clone(),
                    };
                    let rpc = self.rpc.clone();
                    let login_status = Arc::clone(&self.login_status);
                    tokio::spawn(async move {
                        let result = start_browser_login(rpc).await;
                        let mut status = login_status.lock().await;
                        *status = match &result {
                            Ok(pending) => LoginStatus::Pending(pending.clone()),
                            Err(_) => LoginStatus::Idle,
                        };
                        outcome_tx.send_replace(Some(result));
                    });
                    outcome_rx
                }
            }
        };

        loop {
            if let Some(result) = outcome.borrow().clone() {
                return result.map(|pending| pending.as_account_state());
            }
            outcome
                .changed()
                .await
                .map_err(|_| BackendError::RpcDisconnected)?;
        }
    }

    pub async fn handle_notification(
        &self,
        note: &RpcNotification,
    ) -> BackendResult<Option<AccountState>> {
        if note.method != "account/login/completed" {
            return Ok(None);
        }
        let Some(login_id) = note.params.get("loginId").and_then(Value::as_str) else {
            return Ok(None);
        };

        let success = note.params.get("success").and_then(Value::as_bool) == Some(true);
        if !self.take_matching_pending(login_id).await {
            return Ok(None);
        }

        if success {
            self.read().await.map(Some)
        } else {
            Ok(Some(AccountState::LoginRequired))
        }
    }

    async fn take_matching_pending(&self, login_id: &str) -> bool {
        loop {
            let mut outcome = {
                let mut status = self.login_status.lock().await;
                match &*status {
                    LoginStatus::Idle => return false,
                    LoginStatus::Pending(pending) if pending.login_id == login_id => {
                        *status = LoginStatus::Idle;
                        return true;
                    }
                    LoginStatus::Pending(_) => return false,
                    LoginStatus::Starting { outcome } => outcome.clone(),
                }
            };

            loop {
                if let Some(result) = outcome.borrow().clone() {
                    if result.is_err() {
                        return false;
                    }
                    break;
                }
                if outcome.changed().await.is_err() {
                    return false;
                }
            }
        }
    }
}

impl PendingLogin {
    fn as_account_state(&self) -> AccountState {
        AccountState::LoginPending {
            login_id: self.login_id.clone(),
            auth_url: self.auth_url.clone(),
        }
    }
}

async fn start_browser_login<T: RpcTransport>(rpc: T) -> BackendResult<PendingLogin> {
    let response = rpc
        .request_value(
            "account/login/start",
            Some(json!({
                "type": "chatgpt",
                "useHostedLoginSuccessPage": true,
                "appBrand": "codex"
            })),
        )
        .await?;
    let login_id = required_string(&response, "loginId", "account_login_start_response")?;
    let auth_url = required_string(&response, "authUrl", "account_login_start_response")?;
    Ok(PendingLogin { login_id, auth_url })
}

fn required_string(response: &Value, field: &str, category: &'static str) -> BackendResult<String> {
    response
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| BackendError::InvalidMessage(category.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{AccountSession, AccountState, LoginStatus, PendingLogin, RpcTransport};
    use crate::backend::{BackendResult, RpcNotification};
    use serde_json::{json, Value};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex as StdMutex};
    use tokio::sync::{watch, Notify, Semaphore};

    #[derive(Clone)]
    struct MockTransport {
        account_reads: Arc<AtomicUsize>,
        login_starts: Arc<AtomicUsize>,
        account_response: Arc<StdMutex<Value>>,
        block_login: bool,
        login_started: Arc<Notify>,
        login_release: Arc<Semaphore>,
    }

    impl Default for MockTransport {
        fn default() -> Self {
            Self {
                account_reads: Arc::new(AtomicUsize::new(0)),
                login_starts: Arc::new(AtomicUsize::new(0)),
                account_response: Arc::new(StdMutex::new(json!({
                    "account": {
                        "type": "chatgpt",
                        "email": "never-retain@example.com",
                        "planType": "plus"
                    },
                    "requiresOpenaiAuth": true
                }))),
                block_login: false,
                login_started: Arc::new(Notify::new()),
                login_release: Arc::new(Semaphore::new(0)),
            }
        }
    }

    impl MockTransport {
        fn with_account_response(response: Value) -> Self {
            Self {
                account_response: Arc::new(StdMutex::new(response)),
                ..Self::default()
            }
        }

        fn blocked_login() -> Self {
            Self {
                block_login: true,
                ..Self::default()
            }
        }

        fn account_reads(&self) -> usize {
            self.account_reads.load(Ordering::SeqCst)
        }

        fn login_starts(&self) -> usize {
            self.login_starts.load(Ordering::SeqCst)
        }

        async fn wait_for_login_start(&self) {
            if self.login_starts() == 0 {
                self.login_started.notified().await;
            }
        }

        fn release_login(&self) {
            self.login_release.add_permits(1);
        }
    }

    #[async_trait::async_trait]
    impl RpcTransport for MockTransport {
        async fn request_value(
            &self,
            method: &'static str,
            params: Option<Value>,
        ) -> BackendResult<Value> {
            match method {
                "account/read" => {
                    self.account_reads.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(params, Some(json!({"refreshToken": false})));
                    Ok(self.account_response.lock().unwrap().clone())
                }
                "account/login/start" => {
                    let sequence = self.login_starts.fetch_add(1, Ordering::SeqCst) + 1;
                    self.login_started.notify_one();
                    assert_eq!(
                        params,
                        Some(json!({
                            "type": "chatgpt",
                            "useHostedLoginSuccessPage": true,
                            "appBrand": "codex"
                        }))
                    );
                    if self.block_login {
                        self.login_release.acquire().await.unwrap().forget();
                    }
                    Ok(json!({
                        "type": "chatgpt",
                        "loginId": format!("login-{sequence}"),
                        "authUrl": format!("https://auth.invalid/{sequence}")
                    }))
                }
                _ => unreachable!("unexpected RPC method: {method}"),
            }
        }
    }

    #[tokio::test]
    async fn chatgpt_account_is_ready_without_retaining_email() {
        let rpc = MockTransport::default();
        let session = AccountSession::new(rpc.clone());

        let state = session.read().await.unwrap();

        assert_eq!(
            state,
            AccountState::Ready {
                plan_type: Some("plus".to_string())
            }
        );
        assert_eq!(rpc.account_reads(), 1);
        assert!(!format!("{session:?}").contains("never-retain@example.com"));
    }

    #[tokio::test]
    async fn starts_only_one_pending_login() {
        let rpc = MockTransport::default();
        let session = AccountSession::new(rpc.clone());

        let first = session.begin_browser_login().await.unwrap();
        let second = session.begin_browser_login().await.unwrap();

        assert_eq!(first, second);
        assert_eq!(rpc.login_starts(), 1);
    }

    #[tokio::test]
    async fn concurrent_callers_share_one_in_flight_login() {
        let rpc = MockTransport::blocked_login();
        let session = Arc::new(AccountSession::new(rpc.clone()));
        let first_session = Arc::clone(&session);
        let first = tokio::spawn(async move { first_session.begin_browser_login().await });
        rpc.wait_for_login_start().await;
        let second_session = Arc::clone(&session);
        let second = tokio::spawn(async move { second_session.begin_browser_login().await });

        tokio::task::yield_now().await;
        assert_eq!(rpc.login_starts(), 1);
        rpc.release_login();

        assert_eq!(
            first.await.unwrap().unwrap(),
            second.await.unwrap().unwrap()
        );
        assert_eq!(rpc.login_starts(), 1);
    }

    #[tokio::test]
    async fn cancelling_first_caller_does_not_cancel_or_duplicate_login_start() {
        let rpc = MockTransport::blocked_login();
        let session = Arc::new(AccountSession::new(rpc.clone()));
        let first_session = Arc::clone(&session);
        let first = tokio::spawn(async move { first_session.begin_browser_login().await });
        rpc.wait_for_login_start().await;
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());

        let retry_session = Arc::clone(&session);
        let retry = tokio::spawn(async move { retry_session.begin_browser_login().await });
        let duplicate = tokio::time::timeout(std::time::Duration::from_millis(100), async {
            while rpc.login_starts() == 1 {
                tokio::task::yield_now().await;
            }
        })
        .await;

        assert!(duplicate.is_err(), "retry started a duplicate login RPC");
        assert_eq!(rpc.login_starts(), 1);
        rpc.release_login();
        assert!(matches!(
            retry.await.unwrap().unwrap(),
            AccountState::LoginPending { .. }
        ));
    }

    #[tokio::test]
    async fn notification_waiting_for_login_start_does_not_hold_mutex() {
        let rpc = MockTransport::blocked_login();
        let session = Arc::new(AccountSession::new(rpc.clone()));
        let start_session = Arc::clone(&session);
        let start = tokio::spawn(async move { start_session.begin_browser_login().await });
        rpc.wait_for_login_start().await;

        let notification_session = Arc::clone(&session);
        let unrelated = tokio::spawn(async move {
            notification_session
                .handle_notification(&RpcNotification {
                    method: "account/login/completed".to_string(),
                    params: json!({"loginId": "old-login", "success": true, "error": null}),
                })
                .await
        });

        let status = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            session.login_status.lock(),
        )
        .await
        .expect("notification held login mutex while waiting for start outcome");
        drop(status);

        rpc.release_login();
        start.await.unwrap().unwrap();
        assert_eq!(unrelated.await.unwrap().unwrap(), None);
    }

    #[tokio::test]
    async fn matching_login_completion_rereads_account() {
        let rpc = MockTransport::default();
        let session = AccountSession::new(rpc.clone());
        session.begin_browser_login().await.unwrap();

        let state = session
            .handle_notification(&RpcNotification {
                method: "account/login/completed".to_string(),
                params: json!({"loginId": "login-1", "success": true, "error": null}),
            })
            .await
            .unwrap();

        assert_eq!(
            state,
            Some(AccountState::Ready {
                plan_type: Some("plus".to_string())
            })
        );
        assert_eq!(rpc.account_reads(), 1);
    }

    #[tokio::test]
    async fn matching_success_waits_for_start_response_to_commit_pending() {
        let rpc = MockTransport::default();
        let session = Arc::new(AccountSession::new(rpc.clone()));
        let (outcome_tx, outcome_rx) = watch::channel(None);
        *session.login_status.lock().await = LoginStatus::Starting {
            outcome: outcome_rx,
        };
        let notification_session = Arc::clone(&session);
        let mut notification = tokio::spawn(async move {
            notification_session
                .handle_notification(&RpcNotification {
                    method: "account/login/completed".to_string(),
                    params: json!({"loginId": "login-race", "success": true, "error": null}),
                })
                .await
        });

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), &mut notification)
                .await
                .is_err(),
            "matching completion was discarded while login state was Starting"
        );
        let pending = PendingLogin {
            login_id: "login-race".to_string(),
            auth_url: "https://auth.invalid/race".to_string(),
        };
        *session.login_status.lock().await = LoginStatus::Pending(pending.clone());
        outcome_tx.send_replace(Some(Ok(pending)));

        assert_eq!(
            notification.await.unwrap().unwrap(),
            Some(AccountState::Ready {
                plan_type: Some("plus".to_string())
            })
        );
        assert_eq!(rpc.account_reads(), 1);
    }

    #[tokio::test]
    async fn matching_failure_waits_for_start_response_without_reading_account() {
        let rpc = MockTransport::default();
        let session = Arc::new(AccountSession::new(rpc.clone()));
        let (outcome_tx, outcome_rx) = watch::channel(None);
        *session.login_status.lock().await = LoginStatus::Starting {
            outcome: outcome_rx,
        };
        let notification_session = Arc::clone(&session);
        let mut notification = tokio::spawn(async move {
            notification_session
                .handle_notification(&RpcNotification {
                    method: "account/login/completed".to_string(),
                    params: json!({
                        "loginId": "login-race",
                        "success": false,
                        "error": "must-not-be-retained"
                    }),
                })
                .await
        });

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), &mut notification)
                .await
                .is_err(),
            "matching failure was discarded while login state was Starting"
        );
        let pending = PendingLogin {
            login_id: "login-race".to_string(),
            auth_url: "https://auth.invalid/race".to_string(),
        };
        *session.login_status.lock().await = LoginStatus::Pending(pending.clone());
        outcome_tx.send_replace(Some(Ok(pending)));

        let state = notification.await.unwrap().unwrap();
        assert_eq!(state, Some(AccountState::LoginRequired));
        assert_eq!(rpc.account_reads(), 0);
        assert!(!format!("{state:?} {session:?}").contains("must-not-be-retained"));
    }

    #[tokio::test]
    async fn failed_login_completion_does_not_read_and_allows_retry_without_error_leak() {
        let rpc = MockTransport::default();
        let session = AccountSession::new(rpc.clone());
        let pending = session.begin_browser_login().await.unwrap();
        let official_error = "official-secret-error-text";

        let state = session
            .handle_notification(&RpcNotification {
                method: "account/login/completed".to_string(),
                params: json!({
                    "loginId": "login-1",
                    "success": false,
                    "error": official_error
                }),
            })
            .await
            .unwrap();

        assert_eq!(state, Some(AccountState::LoginRequired));
        assert_eq!(rpc.account_reads(), 0);
        assert!(!format!("{state:?} {session:?}").contains(official_error));
        let retried = session.begin_browser_login().await.unwrap();
        assert!(matches!(retried, AccountState::LoginPending { .. }));
        assert_eq!(rpc.login_starts(), 2);
        assert_ne!(pending, retried);
    }

    #[tokio::test]
    async fn api_key_local_and_null_accounts_are_not_ready() {
        let responses = [
            json!({"account": {"type": "apiKey"}, "requiresOpenaiAuth": true}),
            json!({"account": null, "requiresOpenaiAuth": false}),
            json!({"account": null, "requiresOpenaiAuth": true}),
        ];

        for response in responses {
            let session = AccountSession::new(MockTransport::with_account_response(response));
            assert_eq!(session.read().await.unwrap(), AccountState::LoginRequired);
        }
    }

    #[tokio::test]
    async fn login_pending_debug_redacts_auth_url() {
        let state = AccountState::LoginPending {
            login_id: "login-1".to_string(),
            auth_url: "https://auth.invalid/secret-token".to_string(),
        };

        let debug = format!("{state:?}");
        assert!(debug.contains("login-1"));
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("https://"));
        assert!(!debug.contains("secret-token"));
    }

    #[tokio::test]
    async fn old_login_completion_is_ignored() {
        let rpc = MockTransport::default();
        let session = AccountSession::new(rpc.clone());
        session.begin_browser_login().await.unwrap();
        session
            .handle_notification(&RpcNotification {
                method: "account/login/completed".to_string(),
                params: json!({"loginId": "login-1", "success": false, "error": "cancelled"}),
            })
            .await
            .unwrap();
        session.begin_browser_login().await.unwrap();
        let reads_before_old_completion = rpc.account_reads();

        let state = session
            .handle_notification(&RpcNotification {
                method: "account/login/completed".to_string(),
                params: json!({"loginId": "login-1", "success": true, "error": null}),
            })
            .await
            .unwrap();

        assert_eq!(state, None);
        assert_eq!(rpc.account_reads(), reads_before_old_completion);
        assert_eq!(rpc.login_starts(), 2);
    }
}
