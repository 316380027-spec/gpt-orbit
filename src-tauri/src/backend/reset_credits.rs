use super::{BackendError, BackendResult};
use chrono::DateTime;
use reqwest::redirect::Policy;
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::Value;
use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::{Host, Url};

const PRODUCTION_ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits";
const MAX_RESPONSE_BYTES: usize = 65_536;
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResetCreditState {
    pub available_count: Option<u32>,
    pub fetched_at: i64,
    pub stale: bool,
    #[serde(default)]
    pub auth_required: bool,
}

#[derive(Clone)]
pub struct ResetCreditAuth {
    access_token: String,
    account_id: String,
}

impl fmt::Debug for ResetCreditAuth {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResetCreditAuth")
            .field("access_token", &"<redacted>")
            .field("account_id", &"<redacted>")
            .finish()
    }
}

#[derive(Deserialize)]
struct AuthFile {
    tokens: AuthTokens,
}

#[derive(Deserialize)]
struct AuthTokens {
    access_token: String,
    account_id: String,
}

impl ResetCreditAuth {
    pub fn load(path: &Path) -> BackendResult<Self> {
        let bytes = std::fs::read(path)
            .map_err(|_| BackendError::RpcError("reset_credit_auth_read".to_string()))?;
        let auth = serde_json::from_slice::<AuthFile>(&bytes)
            .map_err(|_| BackendError::InvalidMessage("reset_credit_auth".to_string()))?;
        if auth.tokens.access_token.is_empty() || auth.tokens.account_id.is_empty() {
            return Err(BackendError::InvalidMessage(
                "reset_credit_auth".to_string(),
            ));
        }
        Ok(Self {
            access_token: auth.tokens.access_token,
            account_id: auth.tokens.account_id,
        })
    }
}

pub fn normalize_reset_credit_response(
    response: Value,
    now: i64,
) -> BackendResult<ResetCreditState> {
    let object = response.as_object().ok_or_else(invalid_response)?;
    let available_count = if let Some(value) = object.get("available_count") {
        let value = value.as_u64().ok_or_else(invalid_response)?;
        Some(u32::try_from(value).map_err(|_| invalid_response())?)
    } else {
        let credits = object
            .get("credits")
            .and_then(Value::as_array)
            .ok_or_else(invalid_response)?;
        let mut count = 0_u32;
        for credit in credits {
            let credit = credit.as_object().ok_or_else(invalid_response)?;
            let status = credit
                .get("status")
                .and_then(Value::as_str)
                .ok_or_else(invalid_response)?;
            if status != "available" {
                continue;
            }
            let expires_at = credit
                .get("expires_at")
                .and_then(Value::as_str)
                .ok_or_else(invalid_response)?;
            let expires_at =
                DateTime::parse_from_rfc3339(expires_at).map_err(|_| invalid_response())?;
            if expires_at.timestamp() > now {
                count = count.checked_add(1).ok_or_else(invalid_response)?;
            }
        }
        Some(count)
    };

    Ok(ResetCreditState {
        available_count,
        fetched_at: now,
        stale: false,
        auth_required: false,
    })
}

fn invalid_response() -> BackendError {
    BackendError::InvalidMessage("reset_credit_response".to_string())
}

#[async_trait::async_trait]
pub trait ResetCreditTransport: Send + Sync {
    async fn fetch(&self) -> BackendResult<ResetCreditState>;
}

#[derive(Clone)]
pub struct ResetCreditClient {
    http: reqwest::Client,
    endpoint: Url,
    auth_path: PathBuf,
}

impl ResetCreditClient {
    pub fn production(auth_path: PathBuf) -> BackendResult<Self> {
        let endpoint = Url::parse(PRODUCTION_ENDPOINT)
            .map_err(|_| BackendError::RpcError("reset_credit_endpoint".to_string()))?;
        if endpoint.scheme() != "https" || endpoint.host_str() != Some("chatgpt.com") {
            return Err(BackendError::RpcError("reset_credit_endpoint".to_string()));
        }
        Self::build(endpoint, auth_path)
    }

    pub fn with_loopback_endpoint(endpoint: Url, auth_path: PathBuf) -> BackendResult<Self> {
        if !is_allowed_loopback_endpoint(&endpoint) {
            return Err(BackendError::RpcError("reset_credit_endpoint".to_string()));
        }
        Self::build(endpoint, auth_path)
    }

    fn build(endpoint: Url, auth_path: PathBuf) -> BackendResult<Self> {
        let http = reqwest::Client::builder()
            .redirect(Policy::none())
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| BackendError::RpcError("reset_credit_client".to_string()))?;
        Ok(Self {
            http,
            endpoint,
            auth_path,
        })
    }
}

fn is_allowed_loopback_endpoint(endpoint: &Url) -> bool {
    if endpoint.scheme() != "http"
        || endpoint.port().is_none()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.fragment().is_some()
    {
        return false;
    }
    matches!(
        endpoint.host(),
        Some(Host::Ipv4(address)) if address == Ipv4Addr::LOCALHOST
    ) || matches!(
        endpoint.host(),
        Some(Host::Ipv6(address)) if address == Ipv6Addr::LOCALHOST
    )
}

#[async_trait::async_trait]
impl ResetCreditTransport for ResetCreditClient {
    async fn fetch(&self) -> BackendResult<ResetCreditState> {
        let auth = ResetCreditAuth::load(&self.auth_path)?;
        let mut response = self
            .http
            .get(self.endpoint.clone())
            .bearer_auth(&auth.access_token)
            .header("ChatGPT-Account-ID", &auth.account_id)
            .header("OpenAI-Beta", "codex-1")
            .header("Originator", "Gpt Orbit Weekly")
            .send()
            .await
            .map_err(|_| BackendError::RpcError("reset_credit_request".to_string()))?;
        drop(auth);

        if matches!(
            response.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) {
            return Err(BackendError::AuthenticationRequired);
        }
        if !response.status().is_success() {
            return Err(BackendError::RpcError("reset_credit_http".to_string()));
        }

        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| BackendError::RpcError("reset_credit_body".to_string()))?
        {
            let remaining = MAX_RESPONSE_BYTES.saturating_sub(body.len());
            if chunk.len() > remaining {
                return Err(BackendError::RpcError(
                    "reset_credit_body_limit".to_string(),
                ));
            }
            body.extend_from_slice(&chunk);
        }
        let value = serde_json::from_slice(&body).map_err(|_| invalid_response())?;
        normalize_reset_credit_response(value, unix_timestamp_now()?)
    }
}

fn unix_timestamp_now() -> BackendResult<i64> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| BackendError::RpcError("reset_credit_clock".to_string()))?
        .as_secs();
    i64::try_from(seconds).map_err(|_| BackendError::RpcError("reset_credit_clock".to_string()))
}

#[derive(Clone)]
pub struct ResetCreditCache {
    path: PathBuf,
}

impl ResetCreditCache {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub async fn load(&self) -> BackendResult<Option<ResetCreditState>> {
        let bytes = match tokio::fs::read(&self.path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => {
                return Err(BackendError::RpcError(
                    "reset_credit_cache_read".to_string(),
                ))
            }
        };
        let mut state = match serde_json::from_slice::<ResetCreditState>(&bytes) {
            Ok(state) => state,
            Err(_) => {
                tracing::warn!(category = "reset_credit_cache_invalid");
                return Ok(None);
            }
        };
        state.stale = true;
        Ok(Some(state))
    }

    pub async fn store(&self, state: &ResetCreditState) -> BackendResult<()> {
        let bytes = serde_json::to_vec(state)
            .map_err(|_| BackendError::RpcError("reset_credit_cache_encode".to_string()))?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| BackendError::RpcError("reset_credit_cache_path".to_string()))?;
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|_| BackendError::RpcError("reset_credit_cache_write".to_string()))?;
        let temporary = self.path.with_extension(format!(
            "tmp-{}-{}",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        if tokio::fs::write(&temporary, bytes).await.is_err() {
            return Err(BackendError::RpcError(
                "reset_credit_cache_write".to_string(),
            ));
        }
        if tokio::fs::rename(&temporary, &self.path).await.is_err() {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(BackendError::RpcError(
                "reset_credit_cache_write".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn clear(&self) -> BackendResult<()> {
        match tokio::fs::remove_file(&self.path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(BackendError::RpcError(
                "reset_credit_cache_clear".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_reset_credit_response, BackendError, ResetCreditAuth, ResetCreditCache,
        ResetCreditClient, ResetCreditState, ResetCreditTransport, MAX_RESPONSE_BYTES,
    };
    use chrono::DateTime;
    use serde_json::{json, Value};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc::{self, Receiver};
    use std::thread::JoinHandle;
    use std::time::Duration;
    use url::Url;

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    fn temp_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "codex-orbit-reset-credit-{label}-{}-{}.json",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn write_auth(path: &Path, token: &str, account_id: &str) {
        std::fs::write(
            path,
            serde_json::to_vec(&json!({
                "tokens": {
                    "access_token": token,
                    "account_id": account_id
                }
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn read_request(mut stream: &TcpStream) -> Vec<u8> {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = stream.read(&mut buffer).unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
        }
        request
    }

    fn response(status: &str, headers: &[(&str, String)], body: &[u8]) -> Vec<u8> {
        let mut response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
            body.len()
        )
        .into_bytes();
        for (name, value) in headers {
            response.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
        }
        response.extend_from_slice(b"\r\n");
        response.extend_from_slice(body);
        response
    }

    fn spawn_server(response: Vec<u8>) -> (Url, Receiver<Vec<u8>>, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let endpoint = Url::parse(&format!("http://{address}/reset-credits")).unwrap();
        let (request_tx, request_rx) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request(&stream);
            let _ = request_tx.send(request);
            stream.write_all(&response).unwrap();
        });
        (endpoint, request_rx, handle)
    }

    #[test]
    fn explicit_zero_is_preserved_and_malformed_values_are_rejected() {
        let state =
            normalize_reset_credit_response(serde_json::json!({"available_count": 0}), 7).unwrap();
        assert_eq!(state.available_count, Some(0));
        assert_eq!(state.fetched_at, 7);
        for value in [
            serde_json::json!(-1),
            serde_json::json!(1.5),
            serde_json::json!("3"),
        ] {
            assert!(normalize_reset_credit_response(
                serde_json::json!({"available_count": value}),
                7
            )
            .is_err());
        }
    }

    #[test]
    fn credits_array_fallback_counts_only_available_unexpired_entries() {
        let state = normalize_reset_credit_response(
            serde_json::json!({"credits": [
                {"status": "available", "expires_at": "2030-01-01T00:00:00Z"},
                {"status": "redeemed", "expires_at": "2030-01-01T00:00:00Z"},
                {"status": "available", "expires_at": "2020-01-01T00:00:00Z"}
            ]}),
            1_800_000_000,
        )
        .unwrap();
        assert_eq!(state.available_count, Some(1));
    }

    #[test]
    fn top_level_count_takes_precedence_over_credit_array_fallback() {
        let state = normalize_reset_credit_response(
            serde_json::json!({
                "available_count": 2,
                "credits": [
                    {"status": "available", "expires_at": "2030-01-01T00:00:00Z"},
                    {"status": "available", "expires_at": "2030-01-01T00:00:00Z"},
                    {"status": "available", "expires_at": "2030-01-01T00:00:00Z"}
                ]
            }),
            1_800_000_000,
        )
        .unwrap();

        assert_eq!(state.available_count, Some(2));
    }

    #[test]
    fn credit_expiring_exactly_now_is_not_available() {
        let expires_at = DateTime::parse_from_rfc3339("2030-01-01T00:00:00Z")
            .unwrap()
            .timestamp();
        let state = normalize_reset_credit_response(
            serde_json::json!({"credits": [
                {"status": "available", "expires_at": "2030-01-01T00:00:00Z"}
            ]}),
            expires_at,
        )
        .unwrap();

        assert_eq!(state.available_count, Some(0));
    }

    #[test]
    fn auth_debug_redacts_token_and_account_id() {
        let path = temp_path("auth-redaction");
        write_auth(&path, "synthetic-secret-token", "synthetic-account-id");

        let auth = ResetCreditAuth::load(&path).unwrap();
        let debug = format!("{auth:?}");

        let _ = std::fs::remove_file(path);
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("synthetic-secret-token"));
        assert!(!debug.contains("synthetic-account-id"));
    }

    #[test]
    fn loopback_constructor_rejects_non_loopback_or_insecure_shapes() {
        let auth_path = temp_path("host-lock-auth");
        for endpoint in [
            "https://127.0.0.1:4444/reset",
            "http://localhost:4444/reset",
            "http://192.0.2.1:4444/reset",
            "http://127.0.0.1/reset",
        ] {
            assert!(ResetCreditClient::with_loopback_endpoint(
                Url::parse(endpoint).unwrap(),
                auth_path.clone()
            )
            .is_err());
        }
    }

    #[tokio::test]
    async fn loopback_fetch_sends_required_headers_and_normalizes_response() {
        let auth_path = temp_path("headers-auth");
        write_auth(
            &auth_path,
            "synthetic-header-token",
            "synthetic-header-account",
        );
        let (endpoint, request_rx, server) = spawn_server(response(
            "200 OK",
            &[("Content-Type", "application/json".to_string())],
            br#"{"available_count":3}"#,
        ));
        let client =
            ResetCreditClient::with_loopback_endpoint(endpoint, auth_path.clone()).unwrap();

        let state = client.fetch().await.unwrap();
        let request = String::from_utf8(request_rx.recv().unwrap()).unwrap();
        server.join().unwrap();
        let _ = std::fs::remove_file(auth_path);

        assert_eq!(state.available_count, Some(3));
        assert!(!state.stale);
        assert!(request.starts_with("GET /reset-credits HTTP/1.1\r\n"));
        assert!(request.contains("authorization: Bearer synthetic-header-token\r\n"));
        assert!(request.contains("chatgpt-account-id: synthetic-header-account\r\n"));
        assert!(request.contains("openai-beta: codex-1\r\n"));
        assert!(request.contains("originator: Gpt Orbit Weekly\r\n"));
    }

    #[tokio::test]
    async fn loopback_redirect_is_rejected_without_following_location() {
        let auth_path = temp_path("redirect-auth");
        write_auth(&auth_path, "synthetic-token", "synthetic-account");
        let redirect_target = TcpListener::bind("127.0.0.1:0").unwrap();
        redirect_target.set_nonblocking(true).unwrap();
        let location = format!(
            "http://{}/redirected",
            redirect_target.local_addr().unwrap()
        );
        let (endpoint, _, server) =
            spawn_server(response("302 Found", &[("Location", location)], b""));
        let client =
            ResetCreditClient::with_loopback_endpoint(endpoint, auth_path.clone()).unwrap();

        assert!(client.fetch().await.is_err());
        server.join().unwrap();
        std::thread::sleep(Duration::from_millis(20));
        assert!(matches!(
            redirect_target.accept(),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
        ));
        let _ = std::fs::remove_file(auth_path);
    }

    #[tokio::test]
    async fn unauthorized_and_forbidden_are_typed_without_exposing_response_data() {
        for status in ["401 Unauthorized", "403 Forbidden"] {
            let auth_path = temp_path("auth-required");
            write_auth(&auth_path, "synthetic-token", "synthetic-account");
            let (endpoint, _, server) = spawn_server(response(
                status,
                &[("X-Synthetic-Secret", "header-secret".to_string())],
                b"body-secret",
            ));
            let client =
                ResetCreditClient::with_loopback_endpoint(endpoint, auth_path.clone()).unwrap();

            let result = client.fetch().await;
            server.join().unwrap();
            let _ = std::fs::remove_file(auth_path);

            assert!(matches!(result, Err(BackendError::AuthenticationRequired)));
            let rendered = format!("{result:?}");
            for secret in [
                "synthetic-token",
                "synthetic-account",
                "header-secret",
                "body-secret",
            ] {
                assert!(!rendered.contains(secret));
            }
        }
    }

    #[tokio::test]
    async fn response_larger_than_65536_bytes_is_rejected() {
        let auth_path = temp_path("body-limit-auth");
        write_auth(&auth_path, "synthetic-token", "synthetic-account");
        let body = serde_json::to_vec(&json!({
            "available_count": 3,
            "padding": "x".repeat(65_536)
        }))
        .unwrap();
        assert!(body.len() > 65_536);
        let (endpoint, _, server) = spawn_server(response(
            "200 OK",
            &[("Content-Type", "application/json".to_string())],
            &body,
        ));
        let client =
            ResetCreditClient::with_loopback_endpoint(endpoint, auth_path.clone()).unwrap();

        let result = client.fetch().await;
        server.join().unwrap();
        let _ = std::fs::remove_file(auth_path);

        assert!(matches!(
            result,
            Err(BackendError::RpcError(category)) if category == "reset_credit_body_limit"
        ));
    }

    #[tokio::test]
    async fn response_exactly_65536_bytes_is_accepted() {
        let auth_path = temp_path("body-limit-boundary-auth");
        write_auth(&auth_path, "synthetic-token", "synthetic-account");
        let prefix = br#"{"available_count":3,"padding":""#;
        let suffix = br#""}"#;
        let mut body = Vec::with_capacity(MAX_RESPONSE_BYTES);
        body.extend_from_slice(prefix);
        body.extend(std::iter::repeat_n(
            b'x',
            MAX_RESPONSE_BYTES - prefix.len() - suffix.len(),
        ));
        body.extend_from_slice(suffix);
        assert_eq!(body.len(), MAX_RESPONSE_BYTES);
        let (endpoint, _, server) = spawn_server(response(
            "200 OK",
            &[("Content-Type", "application/json".to_string())],
            &body,
        ));
        let client =
            ResetCreditClient::with_loopback_endpoint(endpoint, auth_path.clone()).unwrap();

        let result = client.fetch().await;
        server.join().unwrap();
        let _ = std::fs::remove_file(auth_path);

        assert_eq!(result.unwrap().available_count, Some(3));
    }

    #[tokio::test]
    async fn cache_load_is_stale_and_serialized_keys_are_safe() {
        let path = temp_path("cache");
        let cache = ResetCreditCache::new(path.clone());
        let state = ResetCreditState {
            available_count: Some(4),
            fetched_at: 1_800_000_000,
            stale: false,
            auth_required: false,
        };

        cache.store(&state).await.unwrap();
        let serialized: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let restored = cache.load().await.unwrap().unwrap();

        assert_eq!(
            serialized
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["authRequired", "availableCount", "fetchedAt", "stale"]
        );
        assert_eq!(restored.available_count, Some(4));
        assert_eq!(restored.fetched_at, 1_800_000_000);
        assert!(restored.stale);
        let text = serialized.to_string().to_ascii_lowercase();
        for forbidden in ["token", "account", "authorization", "raw", "response"] {
            assert!(!text.contains(forbidden), "cache contained {forbidden}");
        }

        cache.clear().await.unwrap();
        assert_eq!(cache.load().await.unwrap(), None);
    }

    #[tokio::test]
    async fn cache_accepts_legacy_state_without_auth_required_flag() {
        let path = temp_path("legacy-cache");
        std::fs::write(
            &path,
            br#"{"availableCount":4,"fetchedAt":1800000000,"stale":false}"#,
        )
        .unwrap();

        let restored = ResetCreditCache::new(path.clone())
            .load()
            .await
            .unwrap()
            .unwrap();

        assert_eq!(restored.available_count, Some(4));
        assert!(restored.stale);
        assert!(!restored.auth_required);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn cache_store_replaces_an_existing_snapshot() {
        let path = temp_path("cache-replacement");
        let cache = ResetCreditCache::new(path.clone());
        cache
            .store(&ResetCreditState {
                available_count: Some(4),
                fetched_at: 10,
                stale: false,
                auth_required: false,
            })
            .await
            .unwrap();

        cache
            .store(&ResetCreditState {
                available_count: Some(2),
                fetched_at: 20,
                stale: false,
                auth_required: true,
            })
            .await
            .unwrap();

        let restored = cache.load().await.unwrap().unwrap();
        assert_eq!(restored.available_count, Some(2));
        assert_eq!(restored.fetched_at, 20);
        assert!(restored.auth_required);
        cache.clear().await.unwrap();
    }
}
