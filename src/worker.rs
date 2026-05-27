use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use metrics::{counter, gauge, histogram};
use socket2::{SockRef, TcpKeepalive};
use tokio::net::{TcpListener, TcpStream};
use tokio::select;
use tokio::time::{sleep, timeout};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, instrument, warn};

use crate::health::HealthProbe;
use crate::{balancer as b, domain as d, proxy as p};

// Shared between all the accept loop workers
pub(crate) struct AppContext {
    pub(crate) name: String,
    pub(crate) targets: Vec<d::Target>,
    pub(crate) targets_meta: HashMap<d::Target, d::AppTargetMeta>,
    pub(crate) load_balancer_strategy: Mutex<b::LoadBalancerStrategy>,
}

#[derive(Debug, thiserror::Error)]
enum ProxyError {
    #[error("failed to init connection to target: {0}")]
    InitTargetConnectionFailed(#[source] std::io::Error),
    #[error("failed to configure tcp socket options: {0}: {1}")]
    SocketConfigurationFailed(&'static str, #[source] std::io::Error),
}

#[instrument(
    skip_all,
    fields(
        worker_port = listener.local_addr().unwrap().port(),
    ),
)]
pub(crate) async fn run_accept_loop_worker(
    shutdown: CancellationToken,
    app_context: Arc<AppContext>,
    listener: TcpListener,
    targets_registry: Arc<d::TargetRegistry>,
) {
    info!(
        "started accept loop worker on port {}",
        listener.local_addr().unwrap().port()
    );
    loop {
        select! {
            biased;
            res = listener.accept() => {
                match res {
                    Ok((stream, sock_addr)) => {
                        info!(sock_addr = %sock_addr, "accepted new client stream");
                        let _ = stream.set_nodelay(true)
                            .inspect_err(|e| error!(error = %e, "failed to set TCP_NODELAY for client stream"));
                        // maybe not the best solution as I don't know where to put info!() event
                        // about stopped worker
                        tokio::spawn(shutdown.clone().run_until_cancelled_owned(handle_new_connection(
                            targets_registry.clone(),
                            app_context.clone(),
                            stream,
                        )));
                    },
                    Err(error) => {
                        error!(error = %error, "failed to accept new connection");
                        sleep(Duration::from_millis(100)).await;
                    },
                }
            },
        }
    }
}

#[instrument(
    skip_all,
    fields(
        app = %app_context.name,
        client = %client_stream.peer_addr().unwrap(),
    ),
)]
pub(crate) async fn handle_new_connection(
    targets_registry: Arc<d::TargetRegistry>,
    app_context: Arc<AppContext>,
    mut client_stream: TcpStream,
) {
    let app = &app_context.name;
    counter!("proxy_connections_total", "app" => app.clone()).increment(1);
    gauge!("proxy_connections_active", "app" => app.clone()).increment(1.0);
    let start = Instant::now();

    let mut tried: HashSet<&d::Target> = HashSet::new();

    let mut target_stream = loop {
        let available = app_context
            .targets
            .iter()
            .filter(|t| !tried.contains(t))
            .filter(|t| {
                targets_registry
                    .view(t, |_, meta| meta.health_check.is_healthy())
                    .unwrap_or(true)
            })
            .collect();
        let Some(target) = app_context
            .load_balancer_strategy
            .lock()
            .unwrap()
            .route(&app_context.targets_meta, available)
        else {
            counter!("proxy_targets_exhausted_total", "app" => app.clone()).increment(1);
            warn!("all targets exhausted");
            gauge!("proxy_connections_active", "app" => app.clone()).decrement(1.0);
            histogram!("proxy_connection_duration_seconds", "app" => app.clone())
                .record(start.elapsed().as_secs_f64());
            return;
        };

        tried.insert(target);

        match try_connect(target).await {
            Ok(stream) => {
                if let Some(mut entry) = targets_registry.get_mut(target) {
                    entry.health_check.record_success();
                }
                break stream;
            }
            Err(e) => {
                counter!("proxy_connect_failures_total", "app" => app.clone(), "target" => target.as_ref().to_owned())
                    .increment(1);
                warn!(target = ?target, error = %e, "connect failed, trying next");
                if let Some(mut entry) = targets_registry.get_mut(target) {
                    entry.health_check.record_failure();
                }
            }
        }
    };

    match p::handle_stream(&mut client_stream, &mut target_stream).await {
        Ok((to_target_bytes, to_client_bytes)) => {
            counter!("proxy_bytes_transferred", "app" => app.clone(), "direction" => "to_target")
                .increment(to_target_bytes);
            counter!("proxy_bytes_transferred", "app" => app.clone(), "direction" => "to_client")
                .increment(to_client_bytes);
            debug!(
                bytes_out = to_target_bytes,
                bytes_in = to_client_bytes,
                "stream closed"
            );
        }
        Err(e) => {
            counter!("proxy_stream_errors_total", "app" => app.clone()).increment(1);
            warn!(error = %e, "stream error")
        }
    };

    histogram!("proxy_connection_duration_seconds", "app" => app.clone())
        .record(start.elapsed().as_secs_f64());
    gauge!("proxy_connections_active", "app" => app.clone()).decrement(1.0);
}

async fn try_connect(target: &d::Target) -> Result<TcpStream, ProxyError> {
    // TODO: design some kind of total timeout per-app, so we can approximately give equal
    // opportunities to different candidates
    const TIMEOUT: Duration = Duration::from_secs(5);

    let target_stream: TcpStream = timeout(TIMEOUT, TcpStream::connect(target.as_ref()))
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                format!("timed out trying to init connection: {TIMEOUT:?}"),
            )
        })
        .map_err(ProxyError::InitTargetConnectionFailed)?
        .map_err(ProxyError::InitTargetConnectionFailed)?;

    target_stream
        .set_nodelay(true)
        .map_err(|e| ProxyError::SocketConfigurationFailed("failed to enable TCP_NODELAY", e))?;

    // TODO: parameterize in config
    let keepalive = TcpKeepalive::new()
        .with_time(Duration::from_secs(60))
        .with_interval(Duration::from_secs(10))
        .with_retries(3);
    SockRef::from(&target_stream)
        .set_tcp_keepalive(&keepalive)
        .map_err(|e| ProxyError::SocketConfigurationFailed("failed to enable tcp keepalive", e))?;

    Ok(target_stream)
}
