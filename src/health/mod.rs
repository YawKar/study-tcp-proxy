mod passive_probe;
mod tcp_connect_probe;
mod tcp_send_expect_probe;
use std::fmt::Debug;
use std::pin::Pin;
use std::time::Instant;

use tokio::net::ToSocketAddrs;

use crate::config::{self as cfg, HealthCheckStrategyConfig};
use crate::health::passive_probe::PassiveProbe;
use crate::health::tcp_connect_probe::TcpConnectProbe;
use crate::health::tcp_send_expect_probe::TcpSendExpectProbe;

pub(crate) enum HealthCheck {
    Passive(passive_probe::PassiveProbe),
    TcpConnect(tcp_connect_probe::TcpConnectProbe),
    TcpSendExpect(tcp_send_expect_probe::TcpSendExpectProbe),
}

pub(crate) enum ProbeResult {
    Success(Instant),
    Failure(Instant),
}

pub(crate) type Probe = Pin<Box<dyn Future<Output = ProbeResult> + Send>>;

/// Trait for different kinds of probes to be used by HealthCheck
pub(crate) trait HealthProbe {
    /// A very fast method that is used mostly for filtering targets.
    fn is_healthy(&self) -> bool;
    fn needs_probe(&self) -> bool;
    fn setup_probe<Addr: ToSocketAddrs + Sync + Debug + AsRef<str> + Send + 'static>(
        &self,
        target: Addr,
    ) -> Probe;
    /// Needs to be called each time a target acted unavailable.
    fn record_failure(&mut self);
    /// Needs to be called each time a target acted available.
    fn record_success(&mut self);
    fn record_probe(&mut self, instant: Instant);
}

impl HealthProbe for HealthCheck {
    fn is_healthy(&self) -> bool {
        match self {
            HealthCheck::Passive(probe) => probe.is_healthy(),
            HealthCheck::TcpConnect(probe) => probe.is_healthy(),
            HealthCheck::TcpSendExpect(probe) => probe.is_healthy(),
        }
    }

    fn needs_probe(&self) -> bool {
        match self {
            HealthCheck::Passive(probe) => probe.needs_probe(),
            HealthCheck::TcpConnect(probe) => probe.needs_probe(),
            HealthCheck::TcpSendExpect(probe) => probe.needs_probe(),
        }
    }

    fn setup_probe<Addr: ToSocketAddrs + Send + Sync + Debug + AsRef<str> + 'static>(
        &self,
        target: Addr,
    ) -> Probe {
        match self {
            HealthCheck::Passive(probe) => probe.setup_probe(target),
            HealthCheck::TcpConnect(probe) => probe.setup_probe(target),
            HealthCheck::TcpSendExpect(probe) => probe.setup_probe(target),
        }
    }

    fn record_failure(&mut self) {
        match self {
            HealthCheck::Passive(probe) => probe.record_failure(),
            HealthCheck::TcpConnect(probe) => probe.record_failure(),
            HealthCheck::TcpSendExpect(probe) => probe.record_failure(),
        }
    }

    fn record_success(&mut self) {
        match self {
            HealthCheck::Passive(probe) => probe.record_success(),
            HealthCheck::TcpConnect(probe) => probe.record_success(),
            HealthCheck::TcpSendExpect(probe) => probe.record_success(),
        }
    }

    fn record_probe(&mut self, instant: Instant) {
        match self {
            HealthCheck::Passive(probe) => probe.record_probe(instant),
            HealthCheck::TcpConnect(probe) => probe.record_probe(instant),
            HealthCheck::TcpSendExpect(probe) => probe.record_probe(instant),
        }
    }
}

impl TryFrom<&cfg::HealthCheckStrategyConfig> for HealthCheck {
    type Error = anyhow::Error;
    fn try_from(config: &cfg::HealthCheckStrategyConfig) -> Result<HealthCheck, Self::Error> {
        match config {
            HealthCheckStrategyConfig::Passive { .. } => {
                PassiveProbe::try_from(config).map(HealthCheck::Passive)
            }
            HealthCheckStrategyConfig::TcpConnect { .. } => {
                TcpConnectProbe::try_from(config).map(HealthCheck::TcpConnect)
            }
            HealthCheckStrategyConfig::TcpSendExpect { .. } => {
                TcpSendExpectProbe::try_from(config).map(HealthCheck::TcpSendExpect)
            }
        }
    }
}
