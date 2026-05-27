use std::fmt::Debug;
use std::io;
use std::time::{Duration, Instant};

use anyhow::anyhow;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, ToSocketAddrs};
use tokio::time::timeout;
use tracing::{debug, warn};

use crate::config::HealthCheckStrategyConfig;
use crate::health::{HealthProbe, ProbeResult};

pub struct TcpSendExpectProbe {
    // # Config part:
    // How many failures should be registered before marking the target as unavailable.
    // (i.e. when consecutive_failures == failure_count_threshold -> set marked_unhealthy_at)
    failure_count_threshold: u32,
    timeout: Duration,
    interval: Duration,
    send: String,
    expect: String,
    // # State part:
    consecutive_failures: u32,
    last_probe_at: Option<Instant>,
}

impl HealthProbe for TcpSendExpectProbe {
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

    fn setup_probe<Addr: ToSocketAddrs + AsRef<str> + Debug + Send + 'static>(
        &self,
        target: Addr,
    ) -> super::Probe
    where
        for<'a> &'a Addr: Send,
    {
        let self_timeout = self.timeout;
        let self_send = self.send.clone();
        let self_expect = self.expect.clone();
        Box::pin(async move {
            let instant = Instant::now();
            let result: Result<String, io::Error> = async {
                let mut stream = timeout(self_timeout, TcpStream::connect(target.as_ref()))
                    .await
                    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "probe timed out"))??;
                let mut answer_buf = vec![0u8; self_expect.len()].into_boxed_slice();
                stream.write_all(self_send.as_bytes()).await?;
                stream.flush().await?;
                stream.read_exact(&mut answer_buf).await?;
                let answer = String::from_utf8(answer_buf.to_vec()).map_err(|format_error| {
                    io::Error::new(io::ErrorKind::InvalidData, format_error.to_string())
                })?;
                Ok(answer)
            }
            .await;

            match result {
                Ok(answer) if answer == self_expect => {
                    debug!(target = ?target, "successful health probe");
                    ProbeResult::Success(instant)
                }
                Ok(answer) => {
                    warn!(target = ?target, expected = %self_expect, got = %answer, "unexpected probe response");
                    ProbeResult::Failure(instant)
                }
                Err(error) => {
                    warn!(target = ?target, error = %error, "health probe failed");
                    ProbeResult::Failure(instant)
                }
            }
        })
    }

    fn record_failure(&mut self) {
        if self.consecutive_failures == self.failure_count_threshold {
            return;
        }
        self.consecutive_failures += 1;
    }

    fn record_success(&mut self) {
        self.consecutive_failures = 0;
    }

    fn record_probe(&mut self, instant: Instant) {
        self.last_probe_at = Some(instant);
    }
}

impl TryFrom<&HealthCheckStrategyConfig> for TcpSendExpectProbe {
    type Error = anyhow::Error;
    fn try_from(config: &HealthCheckStrategyConfig) -> Result<Self, Self::Error> {
        match config {
            HealthCheckStrategyConfig::TcpSendExpect {
                timeout,
                interval,
                failure_count_threshold,
                send,
                expect,
            } => Ok(Self {
                timeout: *timeout,
                interval: *interval,
                failure_count_threshold: *failure_count_threshold,
                send: send.clone(),
                expect: expect.clone(),
                consecutive_failures: 0,
                last_probe_at: None,
            }),
            _ => Err(anyhow!(
                "TcpSendExpectProbe cannot be constructed from the given HealthCheckStrategyConfig"
            )),
        }
    }
}
