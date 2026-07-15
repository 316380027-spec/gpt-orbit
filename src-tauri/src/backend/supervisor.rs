use super::{BackendError, BackendResult, ServiceCommand};
use std::future::Future;
use std::time::Duration;
use tokio::sync::{mpsc, watch, Mutex};

pub(crate) enum AwaitOutcome<T> {
    Complete(T),
    Shutdown,
}

pub(crate) async fn wait_or_shutdown<F, T>(
    commands: &mut mpsc::Receiver<ServiceCommand>,
    pending_refresh: &mut bool,
    operation: F,
) -> AwaitOutcome<T>
where
    F: Future<Output = T>,
{
    tokio::pin!(operation);
    loop {
        tokio::select! {
            result = &mut operation => {
                while let Ok(command) = commands.try_recv() {
                    match command {
                        ServiceCommand::Refresh(_) => *pending_refresh = true,
                        ServiceCommand::Shutdown => return AwaitOutcome::Shutdown,
                    }
                }
                return AwaitOutcome::Complete(result);
            }
            command = commands.recv() => match command {
                Some(ServiceCommand::Refresh(_)) => *pending_refresh = true,
                Some(ServiceCommand::Shutdown) | None => return AwaitOutcome::Shutdown,
            }
        }
    }
}

#[derive(Default)]
pub struct RestartBackoff {
    failures: usize,
}

impl RestartBackoff {
    pub fn next_delay(&mut self) -> Duration {
        let delay = match self.failures {
            0 => 1,
            1 => 2,
            2 => 4,
            3 => 8,
            _ => 30,
        };
        self.failures = self.failures.saturating_add(1);
        Duration::from_secs(delay)
    }

    pub fn reset(&mut self) {
        self.failures = 0;
    }
}

#[derive(Default)]
pub struct StartGate {
    state: Mutex<StartState>,
}

#[derive(Default)]
enum StartState {
    #[default]
    Idle,
    Starting(watch::Receiver<Option<BackendResult<()>>>),
    Started,
}

impl StartGate {
    pub async fn start<F, Fut>(&self, operation: F) -> BackendResult<()>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = BackendResult<()>> + Send + 'static,
    {
        let mut outcome = {
            let mut state = self.state.lock().await;
            match &*state {
                StartState::Started => return Ok(()),
                StartState::Starting(outcome) => outcome.clone(),
                StartState::Idle => {
                    let (outcome_tx, outcome_rx) = watch::channel(None);
                    *state = StartState::Starting(outcome_rx.clone());
                    tokio::spawn(async move {
                        outcome_tx.send_replace(Some(operation().await));
                    });
                    outcome_rx
                }
            }
        };

        loop {
            let result = { outcome.borrow().clone() };
            if let Some(result) = result {
                let mut state = self.state.lock().await;
                *state = if result.is_ok() {
                    StartState::Started
                } else {
                    StartState::Idle
                };
                return result;
            }
            outcome
                .changed()
                .await
                .map_err(|_| BackendError::RpcDisconnected)?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{wait_or_shutdown, AwaitOutcome, RestartBackoff, StartGate};
    use crate::backend::{RefreshReason, ServiceCommand};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::{mpsc, Notify, Semaphore};

    struct CancelFlag(Arc<AtomicUsize>);

    impl Drop for CancelFlag {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn backoff_is_1_2_4_8_30_capped() {
        let mut backoff = RestartBackoff::default();
        let actual = (0..8).map(|_| backoff.next_delay()).collect::<Vec<_>>();
        assert_eq!(
            actual,
            [1, 2, 4, 8, 30, 30, 30, 30].map(Duration::from_secs)
        );
    }

    #[test]
    fn stable_connection_resets_backoff() {
        let mut backoff = RestartBackoff::default();
        assert_eq!(backoff.next_delay(), Duration::from_secs(1));
        assert_eq!(backoff.next_delay(), Duration::from_secs(2));
        backoff.reset();
        assert_eq!(backoff.next_delay(), Duration::from_secs(1));
    }

    #[tokio::test]
    async fn concurrent_start_spawns_one_child() {
        let gate = Arc::new(StartGate::default());
        let spawns = Arc::new(AtomicUsize::new(0));
        let start = |gate: Arc<StartGate>, spawns: Arc<AtomicUsize>| async move {
            gate.start(move || async move {
                spawns.fetch_add(1, Ordering::SeqCst);
                tokio::task::yield_now().await;
                Ok(())
            })
            .await
        };

        let (first, second) = tokio::join!(
            start(Arc::clone(&gate), Arc::clone(&spawns)),
            start(Arc::clone(&gate), Arc::clone(&spawns))
        );

        first.unwrap();
        second.unwrap();
        assert_eq!(spawns.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn refresh_during_controlled_spawn_is_pending_not_cancelling() {
        let (tx, mut rx) = mpsc::channel(8);
        let release = Arc::new(Semaphore::new(0));
        let cancellations = Arc::new(AtomicUsize::new(0));
        let operation_release = Arc::clone(&release);
        let operation_cancellations = Arc::clone(&cancellations);
        let mut pending_refresh = false;
        let operation = async move {
            let _flag = CancelFlag(operation_cancellations);
            operation_release.acquire().await.unwrap().forget();
            7
        };
        let waiter = tokio::spawn(async move {
            let outcome = wait_or_shutdown(&mut rx, &mut pending_refresh, operation).await;
            (outcome, pending_refresh)
        });

        tx.send(ServiceCommand::Refresh(RefreshReason::Manual))
            .await
            .unwrap();
        tokio::task::yield_now().await;
        assert_eq!(cancellations.load(Ordering::SeqCst), 0);
        assert!(!waiter.is_finished());
        release.add_permits(1);

        let (outcome, pending) = waiter.await.unwrap();
        assert!(matches!(outcome, AwaitOutcome::Complete(7)));
        assert!(pending);
    }

    #[tokio::test]
    async fn refresh_during_controlled_backoff_does_not_skip_delay() {
        let (tx, mut rx) = mpsc::channel(8);
        let release_clock = Arc::new(Semaphore::new(0));
        let clock = Arc::clone(&release_clock);
        let mut pending_refresh = false;
        let waiter = tokio::spawn(async move {
            let outcome = wait_or_shutdown(&mut rx, &mut pending_refresh, async move {
                clock.acquire().await.unwrap().forget();
            })
            .await;
            (outcome, pending_refresh)
        });

        tx.send(ServiceCommand::Refresh(RefreshReason::Tray))
            .await
            .unwrap();
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished(), "refresh skipped the backoff clock");
        release_clock.add_permits(1);

        let (outcome, pending) = waiter.await.unwrap();
        assert!(matches!(outcome, AwaitOutcome::Complete(())));
        assert!(pending);
    }

    #[tokio::test]
    async fn shutdown_cancels_blocked_operation_immediately() {
        let (tx, mut rx) = mpsc::channel(8);
        let cancellations = Arc::new(AtomicUsize::new(0));
        let operation_cancellations = Arc::clone(&cancellations);
        let started = Arc::new(Notify::new());
        let operation_started = Arc::clone(&started);
        let mut pending_refresh = false;
        let waiter = tokio::spawn(async move {
            wait_or_shutdown(&mut rx, &mut pending_refresh, async move {
                let _flag = CancelFlag(operation_cancellations);
                operation_started.notify_one();
                std::future::pending::<()>().await;
            })
            .await
        });

        started.notified().await;
        tx.send(ServiceCommand::Shutdown).await.unwrap();
        let outcome = tokio::time::timeout(Duration::from_millis(100), waiter)
            .await
            .expect("shutdown did not cancel blocked operation")
            .unwrap();

        assert!(matches!(outcome, AwaitOutcome::Shutdown));
        assert_eq!(cancellations.load(Ordering::SeqCst), 1);
    }
}
