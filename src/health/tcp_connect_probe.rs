use std::fmt::Debug;
use std::time::{Duration, Instant};

use anyhow::anyhow;
use tokio::net::{TcpStream, ToSocketAddrs};
use tokio::time::timeout;
use tracing::{debug, warn};

use crate::config::HealthCheckStrategyConfig;
use crate::health::{HealthProbe, ProbeResult};

pub struct TcpConnectProbe {
    // # Config part:
    // How many failures should be registered before marking the target as unavailable.
    // (i.e. when consecutive_failures == failure_count_threshold -> set marked_unhealthy_at)
    failure_count_threshold: u32,
    timeout: Duration,
    interval: Duration,

    // # State part:
    consecutive_failures: u32,
    last_probe_at: Option<Instant>,
}

impl HealthProbe for TcpConnectProbe {
    fn is_healthy(&self) -> bool {
        if self.consecutive_failures < self.failure_count_threshold {
            return true;
        }
        debug_assert!(self.consecutive_failures == self.failure_count_threshold);
        false
    }

    fn needs_probe(&self) -> bool {
        match self.last_probe_at {
            Some(last_probe_at) => last_probe_at.elapsed() >= self.interval,
            None => true,
        }
    }

    fn setup_probe<Addr: ToSocketAddrs + Send + Sync + Debug + AsRef<str> + 'static>(
        &self,
        target: Addr,
    ) -> super::Probe
    where
        for<'a> &'a Addr: ToSocketAddrs,
    {
        let self_timeout = self.timeout;
        Box::pin(async move {
            let instant = Instant::now();
            match timeout(self_timeout, TcpStream::connect(target.as_ref())).await {
                Ok(connect_result) => match connect_result {
                    Ok(_) => {
                        debug!(target = ?target, "successful health probe");
                        ProbeResult::Success(instant)
                    }
                    Err(error) => {
                        warn!(target = ?target, error = %error, "health probe failed");
                        ProbeResult::Failure(instant)
                    }
                },
                Err(timeout_error) => {
                    warn!(target = ?target, error = %timeout_error, "health probe failed");
                    ProbeResult::Failure(instant)
                }
            }
        })
    }

    fn record_failure(&mut self) {
        if self.consecutive_failures == self.failure_count_threshold {
            return;
        }
        debug_assert!(self.consecutive_failures < self.failure_count_threshold);
        self.consecutive_failures += 1;
    }

    fn record_success(&mut self) {
        self.consecutive_failures = 0;
    }

    fn record_probe(&mut self, instant: Instant) {
        self.last_probe_at = Some(instant);
    }
}

impl TryFrom<&HealthCheckStrategyConfig> for TcpConnectProbe {
    type Error = anyhow::Error;
    fn try_from(config: &HealthCheckStrategyConfig) -> Result<Self, Self::Error> {
        match config {
            HealthCheckStrategyConfig::TcpConnect {
                timeout,
                interval,
                failure_count_threshold,
            } => Ok(Self {
                timeout: *timeout,
                interval: *interval,
                failure_count_threshold: *failure_count_threshold,
                consecutive_failures: 0,
                last_probe_at: None,
            }),
            _ => Err(anyhow!(
                "TcpConnectProbe cannot be constructed from the given HealthCheckStrategyConfig"
            )),
        }
    }
}
