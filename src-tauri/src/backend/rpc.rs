use super::error::{BackendError, BackendResult};
use super::protocol::{IncomingMessage, NotificationMessage, RequestMessage};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::{broadcast, mpsc, oneshot, watch, Mutex};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Debug)]
pub struct RpcNotification {
    pub method: String,
    pub params: Value,
}

struct RpcInner {
    writer: mpsc::UnboundedSender<String>,
    next_id: AtomicU64,
    pending: Mutex<HashMap<u64, oneshot::Sender<BackendResult<Value>>>>,
    notifications: broadcast::Sender<RpcNotification>,
    disconnected: AtomicBool,
    disconnect_complete: watch::Sender<bool>,
}

async fn disconnect_inner(inner: &Arc<RpcInner>) {
    let mut complete = inner.disconnect_complete.subscribe();
    if !inner.disconnected.swap(true, Ordering::AcqRel) {
        let drain_inner = Arc::clone(inner);
        tokio::spawn(async move {
            let pending = {
                let mut pending = drain_inner.pending.lock().await;
                std::mem::take(&mut *pending)
            };
            for (_, response) in pending {
                let _ = response.send(Err(BackendError::RpcDisconnected));
            }
            drain_inner.disconnect_complete.send_replace(true);
        });
    }

    if *complete.borrow() {
        return;
    }
    while complete.changed().await.is_ok() {
        if *complete.borrow() {
            return;
        }
    }
}

#[derive(Clone)]
pub struct RpcClient {
    inner: Arc<RpcInner>,
}

impl RpcClient {
    pub fn new<W>(mut writer: W) -> Self
    where
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (writer_tx, mut writer_rx) = mpsc::unbounded_channel::<String>();
        let (notifications, _) = broadcast::channel(64);
        let (disconnect_complete, _) = watch::channel(false);
        let inner = Arc::new(RpcInner {
            writer: writer_tx,
            next_id: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
            notifications,
            disconnected: AtomicBool::new(false),
            disconnect_complete,
        });
        let writer_inner = Arc::downgrade(&inner);
        tokio::spawn(async move {
            while let Some(line) = writer_rx.recv().await {
                let result = async {
                    writer.write_all(line.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                    writer.flush().await
                }
                .await;
                if result.is_err() {
                    if let Some(inner) = writer_inner.upgrade() {
                        disconnect_inner(&inner).await;
                    }
                    break;
                }
            }
        });

        Self { inner }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RpcNotification> {
        self.inner.notifications.subscribe()
    }

    pub fn disconnected(&self) -> watch::Receiver<bool> {
        self.inner.disconnect_complete.subscribe()
    }

    pub async fn wait_disconnected(&self) {
        let mut disconnected = self.disconnected();
        while !*disconnected.borrow() {
            if disconnected.changed().await.is_err() {
                return;
            }
        }
    }

    pub async fn request<P, R>(&self, method: &'static str, params: Option<P>) -> BackendResult<R>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        if self.inner.disconnected.load(Ordering::Acquire) {
            return Err(BackendError::RpcDisconnected);
        }

        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let line = serde_json::to_string(&RequestMessage { method, id, params })
            .map_err(|error| BackendError::InvalidMessage(error.to_string()))?;
        let (response_tx, response_rx) = oneshot::channel();
        let mut pending = self.inner.pending.lock().await;
        if self.inner.disconnected.load(Ordering::Acquire) {
            return Err(BackendError::RpcDisconnected);
        }
        pending.insert(id, response_tx);
        drop(pending);

        if self.inner.writer.send(line).is_err() {
            disconnect_inner(&self.inner).await;
            return Err(BackendError::RpcDisconnected);
        }

        let response = match tokio::time::timeout(REQUEST_TIMEOUT, response_rx).await {
            Ok(Ok(response)) => response?,
            Ok(Err(_)) => return Err(BackendError::RpcDisconnected),
            Err(_) => {
                self.inner.pending.lock().await.remove(&id);
                return Err(BackendError::RequestTimeout);
            }
        };

        serde_json::from_value(response)
            .map_err(|error| BackendError::InvalidMessage(error.to_string()))
    }

    pub async fn notify<P>(&self, method: &'static str, params: Option<P>) -> BackendResult<()>
    where
        P: Serialize,
    {
        if self.inner.disconnected.load(Ordering::Acquire) {
            return Err(BackendError::RpcDisconnected);
        }
        let line = serde_json::to_string(&NotificationMessage { method, params })
            .map_err(|error| BackendError::InvalidMessage(error.to_string()))?;
        if self.inner.writer.send(line).is_err() {
            disconnect_inner(&self.inner).await;
            return Err(BackendError::RpcDisconnected);
        }
        Ok(())
    }

    pub async fn accept_line(&self, line: &str) -> BackendResult<()> {
        let message: IncomingMessage = serde_json::from_str(line)
            .map_err(|error| BackendError::InvalidMessage(error.to_string()))?;

        if let Some(id) = message.id {
            if let Some(pending) = self.inner.pending.lock().await.remove(&id) {
                let response = match message.error {
                    Some(error) => Err(BackendError::RpcError(error.to_string())),
                    None => Ok(message.result.unwrap_or(Value::Null)),
                };
                let _ = pending.send(response);
            }
            return Ok(());
        }

        if let Some(method) = message.method {
            let _ = self.inner.notifications.send(RpcNotification {
                method,
                params: message.params.unwrap_or(Value::Null),
            });
            return Ok(());
        }

        Err(BackendError::InvalidMessage(
            "message has neither id nor method".to_string(),
        ))
    }

    pub async fn disconnect(&self) {
        disconnect_inner(&self.inner).await;
    }
}

pub async fn initialize(client: &RpcClient, version: &str) -> BackendResult<()> {
    let _: Value = client
        .request(
            "initialize",
            Some(json!({
                "clientInfo": {
                    "name": "codex_orbit",
                    "title": "Codex Orbit",
                    "version": version
                }
            })),
        )
        .await?;
    client.notify("initialized", Some(json!({}))).await
}

#[cfg(test)]
mod tests {
    use super::{initialize, RpcClient};
    use crate::backend::BackendError;
    use serde_json::{json, Value};
    use std::sync::atomic::Ordering;
    use tokio::io::{duplex, AsyncBufReadExt, BufReader, BufWriter};
    use tokio::sync::oneshot;

    async fn read_message(reader: &mut BufReader<tokio::io::DuplexStream>) -> Value {
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        serde_json::from_str(&line).unwrap()
    }

    #[tokio::test]
    async fn matches_out_of_order_responses() {
        let (client_io, server_io) = duplex(4096);
        let client = RpcClient::new(client_io);
        let mut server = BufReader::new(server_io);

        let first_client = client.clone();
        let first = tokio::spawn(async move {
            first_client
                .request::<_, Value>("first", Some(json!({"value": 1})))
                .await
        });
        let first_message = read_message(&mut server).await;

        let second_client = client.clone();
        let second = tokio::spawn(async move {
            second_client
                .request::<_, Value>("second", Some(json!({"value": 2})))
                .await
        });
        let second_message = read_message(&mut server).await;

        assert_eq!(first_message["id"], 1);
        assert_eq!(second_message["id"], 2);

        client
            .accept_line(r#"{"id":2,"result":{"reply":"second"}}"#)
            .await
            .unwrap();
        client
            .accept_line(r#"{"id":1,"result":{"reply":"first"}}"#)
            .await
            .unwrap();

        assert_eq!(first.await.unwrap().unwrap(), json!({"reply": "first"}));
        assert_eq!(second.await.unwrap().unwrap(), json!({"reply": "second"}));
    }

    #[tokio::test]
    async fn dispatches_idless_notification() {
        let (client_io, _server_io) = duplex(4096);
        let client = RpcClient::new(client_io);
        let mut notifications = client.subscribe();

        client
            .accept_line(r#"{"method":"quota/updated","params":{"remaining":42}}"#)
            .await
            .unwrap();

        let notification = notifications.recv().await.unwrap();
        assert_eq!(notification.method, "quota/updated");
        assert_eq!(notification.params, json!({"remaining": 42}));
    }

    #[tokio::test]
    async fn disconnect_fails_pending_requests() {
        let (client_io, server_io) = duplex(4096);
        let client = RpcClient::new(client_io);
        let mut server = BufReader::new(server_io);

        let request_client = client.clone();
        let pending = tokio::spawn(async move {
            request_client
                .request::<_, Value>("pending", None::<Value>)
                .await
        });
        read_message(&mut server).await;

        client.disconnect().await;

        assert!(matches!(
            pending.await.unwrap(),
            Err(BackendError::RpcDisconnected)
        ));
    }

    #[tokio::test]
    async fn bad_json_does_not_poison_next_line() {
        let (client_io, _server_io) = duplex(4096);
        let client = RpcClient::new(client_io);
        let mut notifications = client.subscribe();

        assert!(matches!(
            client.accept_line("{not-json").await,
            Err(BackendError::InvalidMessage(_))
        ));

        client
            .accept_line(r#"{"method":"still/alive","params":{"ok":true}}"#)
            .await
            .unwrap();
        let notification = notifications.recv().await.unwrap();
        assert_eq!(notification.method, "still/alive");
        assert_eq!(notification.params, json!({"ok": true}));
    }

    #[tokio::test]
    async fn initialized_is_sent_only_after_initialize_response() {
        let (client_io, server_io) = duplex(4096);
        let client = RpcClient::new(client_io);
        let mut server = BufReader::new(server_io);

        let initialize_client = client.clone();
        let handshake = tokio::spawn(async move { initialize(&initialize_client, "1.2.3").await });

        let initialize_message = read_message(&mut server).await;
        assert_eq!(
            initialize_message,
            json!({
                "method": "initialize",
                "id": 1,
                "params": {
                    "clientInfo": {
                        "name": "codex_orbit",
                        "title": "Codex Orbit",
                        "version": "1.2.3"
                    }
                }
            })
        );

        let mut premature_line = String::new();
        assert!(tokio::time::timeout(
            std::time::Duration::from_millis(50),
            server.read_line(&mut premature_line)
        )
        .await
        .is_err());

        client.accept_line(r#"{"id":1,"result":{}}"#).await.unwrap();
        assert_eq!(
            read_message(&mut server).await,
            json!({"method": "initialized", "params": {}})
        );
        handshake.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn flushes_each_jsonl_message() {
        let (client_io, server_io) = duplex(4096);
        let client = RpcClient::new(BufWriter::new(client_io));
        let mut server = BufReader::new(server_io);

        client
            .notify("buffered/message", Some(json!({"ready": true})))
            .await
            .unwrap();

        let message = tokio::time::timeout(
            std::time::Duration::from_millis(250),
            read_message(&mut server),
        )
        .await
        .expect("each complete JSONL message must be flushed");
        assert_eq!(
            message,
            json!({"method": "buffered/message", "params": {"ready": true}})
        );
    }

    #[tokio::test]
    async fn writer_failure_disconnects_and_fails_pending_requests() {
        let (client_io, server_io) = duplex(4096);
        drop(server_io);
        let client = RpcClient::new(client_io);

        let request_client = client.clone();
        let pending = tokio::spawn(async move {
            request_client
                .request::<_, Value>("will/fail", None::<Value>)
                .await
        });
        let result = tokio::time::timeout(std::time::Duration::from_millis(250), pending)
            .await
            .expect("writer failure must fail pending requests immediately")
            .unwrap();
        assert!(matches!(result, Err(BackendError::RpcDisconnected)));

        assert!(matches!(
            client.notify("after/failure", None::<Value>).await,
            Err(BackendError::RpcDisconnected)
        ));
        assert!(matches!(
            client
                .request::<_, Value>("after/failure", None::<Value>)
                .await,
            Err(BackendError::RpcDisconnected)
        ));
    }

    #[tokio::test]
    async fn disconnect_signal_is_retained_after_writer_failure() {
        let (client_io, server_io) = duplex(4096);
        drop(server_io);
        let client = RpcClient::new(client_io);

        let _ = client.notify("force/write", None::<Value>).await;
        tokio::time::timeout(
            std::time::Duration::from_millis(250),
            client.wait_disconnected(),
        )
        .await
        .expect("writer failure must publish disconnect");

        tokio::time::timeout(
            std::time::Duration::from_millis(10),
            client.wait_disconnected(),
        )
        .await
        .expect("late subscribers must observe retained disconnect");
    }

    #[tokio::test]
    async fn concurrent_disconnect_waits_for_pending_drain() {
        let (client_io, _server_io) = duplex(4096);
        let client = RpcClient::new(client_io);
        let (response_tx, response_rx) = oneshot::channel();
        let mut pending_guard = client.inner.pending.lock().await;
        pending_guard.insert(99, response_tx);

        let first_client = client.clone();
        let first = tokio::spawn(async move { first_client.disconnect().await });
        tokio::time::timeout(std::time::Duration::from_millis(250), async {
            while !client.inner.disconnected.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("first disconnect must begin");

        let second_client = client.clone();
        let mut second = tokio::spawn(async move { second_client.disconnect().await });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), &mut second)
                .await
                .is_err(),
            "concurrent disconnect returned before pending requests were drained"
        );

        drop(pending_guard);
        let response = tokio::time::timeout(std::time::Duration::from_millis(250), response_rx)
            .await
            .expect("pending request must be notified")
            .unwrap();
        assert!(matches!(response, Err(BackendError::RpcDisconnected)));
        first.await.unwrap();
        second.await.unwrap();
        assert!(client.inner.pending.lock().await.is_empty());
    }

    #[tokio::test]
    async fn cancelled_disconnect_still_completes_pending_drain() {
        let (client_io, _server_io) = duplex(4096);
        let client = RpcClient::new(client_io);
        let (response_tx, response_rx) = oneshot::channel();
        let mut pending_guard = client.inner.pending.lock().await;
        pending_guard.insert(99, response_tx);

        let disconnect_client = client.clone();
        let disconnect = tokio::spawn(async move { disconnect_client.disconnect().await });
        tokio::time::timeout(std::time::Duration::from_millis(250), async {
            while !client.inner.disconnected.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("disconnect must begin");
        disconnect.abort();
        assert!(disconnect.await.unwrap_err().is_cancelled());

        drop(pending_guard);
        let response = tokio::time::timeout(std::time::Duration::from_millis(250), response_rx)
            .await
            .expect("cancelled caller must not cancel pending drain")
            .unwrap();
        assert!(matches!(response, Err(BackendError::RpcDisconnected)));
        client.disconnect().await;
        assert!(client.inner.pending.lock().await.is_empty());
    }
}
