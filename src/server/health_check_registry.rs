use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures_util::future::join_all;
use tokio::sync::{mpsc, oneshot};

pub type HealthCheckRequest = oneshot::Sender<Result<(), String>>;

#[derive(Default, Clone)]
pub struct HealthCheckRegistry {
    checks: Arc<Mutex<HashMap<&'static str, mpsc::Sender<HealthCheckRequest>>>>,
}

impl HealthCheckRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a new health check with the given name and returns a receiver for health check requests.
    pub fn add_petter(&self, name: &'static str) -> mpsc::Receiver<HealthCheckRequest> {
        let (tx, rx) = mpsc::channel(1);
        self.checks.lock().unwrap().insert(name, tx);
        rx
    }

    /// Checks that all registered health checks respond within the given timeout.
    pub async fn request_pets(&self, timeout: Duration) -> Result<(), &'static str> {
        let checks = self.checks.lock().unwrap().clone();

        let results = join_all(checks.iter().map(|(name, tx)| async move {
            tokio::time::timeout(timeout, async {
                let (reply_tx, reply_rx) = oneshot::channel();
                tx.send(reply_tx)
                    .await
                    .map_err(|_| "check request channel closed".to_string())?;
                reply_rx
                    .await
                    .map_err(|_| "check response channel closed".to_string())?
            })
            .await
            .map_err(|_| format!("timed out after {} milliseconds", timeout.as_millis()))
            .and_then(|result| result)
            .map_err(|reason| {
                tracing::warn!("Health check {:?} failed: {}", name, reason);
                *name
            })
        }))
        .await;

        results.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn respond_once(
        mut requests: mpsc::Receiver<HealthCheckRequest>,
        response: Result<(), String>,
    ) {
        tokio::spawn(async move {
            if let Some(reply_tx) = requests.recv().await {
                let _ = reply_tx.send(response);
            }
        });
    }

    #[tokio::test]
    async fn passing_check_is_healthy() {
        let registry = HealthCheckRegistry::new();
        let requests = registry.add_petter("ok-check");
        respond_once(requests, Ok(()));

        assert_eq!(
            registry.request_pets(Duration::from_millis(50)).await,
            Ok(())
        );
    }

    #[tokio::test]
    async fn failing_check_is_unhealthy() {
        let registry = HealthCheckRegistry::new();
        let requests = registry.add_petter("bad-check");
        respond_once(requests, Err("something broke".to_string()));

        assert_eq!(
            registry.request_pets(Duration::from_millis(50)).await,
            Err("bad-check")
        );
    }

    #[tokio::test]
    async fn unresponsive_check_times_out_as_unhealthy() {
        let registry = HealthCheckRegistry::new();
        let _requests = registry.add_petter("slow-check");
        // Nothing ever answers `_requests`, so the check should time out.

        assert_eq!(
            registry.request_pets(Duration::from_millis(20)).await,
            Err("slow-check")
        );
    }
}
