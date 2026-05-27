use std::time::{Duration, Instant};

use anyhow::anyhow;
use tokio::net::ToSocketAddrs;

use crate::config::HealthCheckStrategyConfig;
use crate::health::HealthProbe;

pub struct PassiveProbe {
    // # Config part:
    // How many failures should be registered before marking the target as unavailable.
    // (i.e. when consecutive_failures == failure_count_threshold -> set marked_unhealthy_at)
    failure_count_threshold: u32,
    // How much time to wait before marking it healthy again (giving it another chance)
    recovery_timeout: Duration,

    // # State part:
    consecutive_failures: u32,
    // The last time consecutive_failures became equal to failure_count_threshold
    marked_unhealthy_at: Option<Instant>,
}

impl HealthProbe for PassiveProbe {
    fn is_healthy(&self) -> bool {
        if self.consecutive_failures < self.failure_count_threshold {
            return true;
        }
        debug_assert_eq!(self.consecutive_failures, self.failure_count_threshold);
        debug_assert!(
            self.marked_unhealthy_at.is_some(),
            "When the last hit is registered marked_unhealthy_at should be set"
        );
        self.marked_unhealthy_at.unwrap().elapsed() >= self.recovery_timeout
    }

    fn needs_probe(&self) -> bool {
        false
    }

    fn setup_probe<Addr: ToSocketAddrs>(&self, _target: Addr) -> super::Probe {
        unreachable!("passive_probe doesn't have any active probing mechanism");
    }

    fn record_failure(&mut self) {
        debug_assert!(self.consecutive_failures <= self.failure_count_threshold);
        if self.consecutive_failures < self.failure_count_threshold {
            self.consecutive_failures += 1;
        }
        if self.consecutive_failures == self.failure_count_threshold {
            // in this unhealthy state the only caller of `record_failure` should be health checker
            self.marked_unhealthy_at = Some(Instant::now());
        }
        debug_assert!(self.consecutive_failures <= self.failure_count_threshold);
    }

    fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.marked_unhealthy_at = None;
    }

    fn record_probe(&mut self, _instant: Instant) {
        // NOOP: passive probe doesn't need to register active probes results
    }
}

impl TryFrom<&HealthCheckStrategyConfig> for PassiveProbe {
    type Error = anyhow::Error;
    fn try_from(config: &HealthCheckStrategyConfig) -> Result<Self, Self::Error> {
        match config {
            HealthCheckStrategyConfig::Passive {
                failure_count_threshold,
                recovery_timeout,
            } => Ok(Self {
                failure_count_threshold: *failure_count_threshold,
                recovery_timeout: *recovery_timeout,
                consecutive_failures: 0,
                marked_unhealthy_at: None,
            }),
            _ => Err(anyhow!(
                "PassiveProbe cannot be constructed from the given HealthCheckStrategyConfig"
            )),
        }
    }
}
