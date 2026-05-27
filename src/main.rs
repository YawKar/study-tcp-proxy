mod balancer;
mod config;
mod domain;
mod health;
mod proxy;
mod worker;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use arc_swap::ArcSwap;
use domain as d;
use metrics::{counter, gauge};
use metrics_exporter_prometheus::PrometheusBuilder;
use tokio::net::TcpListener;
use tokio::select;
use tokio::signal::unix::{SignalKind, signal};
use tokio_util::sync::CancellationToken;
use tracing::info;
use worker as w;

use crate::health::{HealthProbe, ProbeResult};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();
    const CONFIG_PATH: &str = "config.json";
    let config = Arc::new(ArcSwap::from_pointee(config::load_config(CONFIG_PATH)?));
    config::validate_config(&config.load()).with_context(|| "failed to validate_config")?;
    info!("config is valid");

    PrometheusBuilder::new()
        .with_http_listener(([0, 0, 0, 0], 9090))
        .install()
        .context("failed to install prometheus metrics exporter")?;
    info!("prometheus metrics available on :9090/metrics");

    let mut sigterm_stream =
        signal(SignalKind::terminate()).with_context(|| "failed to create SIGTERM listener")?;
    info!("registered SIGTERM handler");
    let mut sigint_stream =
        signal(SignalKind::interrupt()).with_context(|| "failed to create SIGINT listener")?;
    info!("registered SIGINT handler");

    let targets_registry = Arc::new({
        let targets_registry = d::TargetRegistry::new();
        let config = config.load();
        for app in &config.apps {
            for target in &app.targets {
                targets_registry.entry(target.clone()).or_insert({
                    let health_check = if let Some(target_config) = config.targets.get(target) {
                        health::HealthCheck::try_from(&target_config.health_check)
                            .with_context(|| format!("app '{:?}': failed to create health check for '{}' target", app.name, target.clone().into_inner()))?
                    } else {
                        (&config::HealthCheckStrategyConfig::default()).try_into()
                            .with_context(|| format!("app '{:?}': failed to create default health check for '{}' target", app.name, target.clone().into_inner()))?
                    };

                    d::TargetState { health_check }
                });
            }
        }
        targets_registry
    });

    let shutdown = CancellationToken::new();
    init_accept_loops(shutdown.clone(), targets_registry.clone(), config.clone())
        .await
        .with_context(|| "failed to init accept loops")?;
    info!("initialized accept loops");

    tokio::spawn(init_health_checker(
        targets_registry.clone(),
        shutdown.clone(),
    ));
    info!("initialized health checker");

    let graceful_shutdown = async || {
        shutdown.cancel();
        info!("sent shutdown event to health checker and workers");
        let graceful_shutdown_timeout = Duration::from_secs(10);
        info!("sleeping {graceful_shutdown_timeout:?} before hard shutdown");
        tokio::time::sleep(graceful_shutdown_timeout).await;
    };

    let signal = select! {
        _ = sigterm_stream.recv() => "SIGTERM",
        _ = sigint_stream.recv() => "SIGINT",
    };
    info!("got {signal}, shutting down");
    graceful_shutdown().await;
    Ok(())
}

async fn init_accept_loops(
    shutdown: CancellationToken,
    targets_registry: Arc<d::TargetRegistry>,
    config: Arc<ArcSwap<config::Config>>,
) -> anyhow::Result<()> {
    const INADDR_ANY: &str = "0.0.0.0";
    let snapshot = config.load();
    for app in &snapshot.apps {
        let app_context = Arc::new(w::AppContext {
            name: app.name.clone().into_inner(),
            targets: app.targets.iter().cloned().collect(),
            targets_meta: app.meta.clone(),
            load_balancer_strategy: Mutex::new(
                (&app.load_balancer_strategy).try_into().with_context(|| {
                    format!(
                        "app '{:?}': failed to create load balancer strategy",
                        app.name
                    )
                })?,
            ),
        });
        for &port in &app.ports {
            let listener = TcpListener::bind((INADDR_ANY, port))
                .await
                .with_context(|| {
                    format!(
                        "app '{:?}': failed to bind to '{INADDR_ANY}:{port}'",
                        app.name
                    )
                })?;
            tokio::spawn(w::run_accept_loop_worker(
                shutdown.clone(),
                app_context.clone(),
                listener,
                targets_registry.clone(),
            ));
        }
    }
    Ok(())
}

async fn init_health_checker(
    targets_registry: Arc<d::TargetRegistry>,
    shutdown: CancellationToken,
) {
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        select! {
            biased;
            _ = shutdown.cancelled() => {
                info!("health checker got shutdown event, stopping health checks");
                break;
            },
            _ = ticker.tick() => {
                let probes: Vec<(d::Target, health::Probe)> = targets_registry
                    .iter()
                    .filter(|entry| entry.health_check.needs_probe())
                    .map(|entry| {
                        (
                            entry.key().clone(),
                            entry
                                .health_check
                                .setup_probe(entry.key().clone().into_inner()),
                        )
                    })
                    .collect();
                let results = futures::future::join_all(
                    probes
                        .into_iter()
                        .map(async move |(target, probe)| (target, probe.await)),
                )
                .await;
                for (target, result) in results {
                    if let Some(mut entry) = targets_registry.get_mut(&target) {
                        match result {
                            ProbeResult::Success(instant) => {
                                counter!(
                                    "proxy_health_probes_total",
                                    "target" => target.as_ref().to_owned(),
                                    "result" => "success",
                                ).increment(1);
                                entry.health_check.record_success();
                                entry.health_check.record_probe(instant);
                            }
                            ProbeResult::Failure(instant) => {
                                counter!(
                                    "proxy_health_probes_total",
                                    "target" => target.as_ref().to_owned(),
                                    "result" => "failure",
                                ).increment(1);
                                entry.health_check.record_failure();
                                entry.health_check.record_probe(instant);
                            }
                        }
                        gauge!(
                            "proxy_target_healthy",
                            "target" => target.as_ref().to_owned(),
                        ).set(if entry.health_check.is_healthy() { 1.0 } else { 0.0 });
                    }
                }

            },
        }
    }
}
