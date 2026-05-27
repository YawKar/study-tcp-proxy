use std::collections::{HashMap, HashSet};
use std::time::Duration;

use anyhow::{Context, ensure};

use crate::domain as d;

pub(crate) fn load_config(path: &str) -> anyhow::Result<Config> {
    let config_raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file: {path}"))?;
    let mut deserializer = serde_json::Deserializer::from_str(&config_raw);
    let config: Config = serde_path_to_error::deserialize(&mut deserializer)?;
    Ok(config)
}

pub(crate) fn validate_config(config: &Config) -> anyhow::Result<()> {
    fn validate_app_config(app: &AppConfig) -> anyhow::Result<()> {
        ensure!(
            !app.ports.is_empty(),
            "app must have at least 1 listening port"
        );
        ensure!(!app.targets.is_empty(), "app must have at least 1 target");
        Ok(())
    }
    fn validate_reserved_ports(
        apps_by_reserved_port: &HashMap<u16, Vec<&d::AppName>>,
    ) -> anyhow::Result<()> {
        ensure!(
            apps_by_reserved_port.is_empty(),
            "the following ports are not exclusive to any single app:\n{:#?}",
            apps_by_reserved_port
        );
        Ok(())
    }
    fn validate_apps_config(apps: &Vec<AppConfig>) -> anyhow::Result<()> {
        let mut apps_by_reserved_port = HashMap::with_capacity(apps.len());
        let mut app_names_count: HashMap<String, i32> = HashMap::with_capacity(apps.len());

        ensure!(!apps.is_empty(), "config must have at least one app");
        for app in apps {
            app_names_count
                .entry(app.name.clone().into_inner())
                .and_modify(|counter| *counter += 1)
                .or_insert(1);
            validate_app_config(app)
                .with_context(|| format!("failed to validate '{:?}' app config", app.name))?;
            for port in &app.ports {
                apps_by_reserved_port
                    .entry(*port)
                    .and_modify(|apps_using: &mut Vec<&d::AppName>| apps_using.push(&app.name))
                    .or_insert(vec![&app.name]);
            }
        }
        apps_by_reserved_port.retain(|_, apps_using| apps_using.len() > 1);
        validate_reserved_ports(&apps_by_reserved_port)
            .with_context(|| "failed to validate exclusivity of reserved ports")?;
        app_names_count.retain(|_, count| *count > 1);
        ensure!(
            app_names_count.is_empty(),
            "the following apps are mentioned multiple times:\n{:#?}",
            app_names_count,
        );
        Ok(())
    }

    fn validate_targets_config(targets: &HashMap<d::Target, TargetConfig>) -> anyhow::Result<()> {
        fn validate_health_check(health_check: &HealthCheckStrategyConfig) -> anyhow::Result<()> {
            match health_check {
                HealthCheckStrategyConfig::Passive {
                    failure_count_threshold,
                    recovery_timeout,
                } => {
                    ensure!(
                        *failure_count_threshold > 0,
                        "failure_count_threshold should be a positive number"
                    );
                    ensure!(
                        !recovery_timeout.is_zero(),
                        "recovery_timeout should not be zero"
                    );
                }
                HealthCheckStrategyConfig::TcpConnect {
                    timeout,
                    interval,
                    failure_count_threshold,
                } => {
                    ensure!(
                        *failure_count_threshold > 0,
                        "failure_count_threshold should be a positive number"
                    );
                    ensure!(!timeout.is_zero(), "timeout should not be zero");
                    ensure!(!interval.is_zero(), "interval should not be zero");
                }
                HealthCheckStrategyConfig::TcpSendExpect {
                    timeout,
                    interval,
                    failure_count_threshold,
                    send,
                    expect,
                } => {
                    ensure!(
                        *failure_count_threshold > 0,
                        "failure_count_threshold should be a positive number"
                    );
                    ensure!(!timeout.is_zero(), "timeout should not be zero");
                    ensure!(!interval.is_zero(), "interval should not be zero");
                    ensure!(
                        !send.is_empty(),
                        "send message should not be empty (maybe you want to use TcpConnect health-check instead?)"
                    );
                    ensure!(
                        !expect.is_empty(),
                        "expect message should not be empty (maybe you want to use TcpConnect health-check instead?)"
                    );
                }
            }
            Ok(())
        }
        for (target, target_config) in targets {
            validate_health_check(&target_config.health_check)
                .with_context(|| format!("invalid health check in '{:?}' target", target))?;
        }
        Ok(())
    }

    validate_apps_config(&config.apps).with_context(|| "failed to validate apps")?;
    validate_targets_config(&config.targets).with_context(|| "failed to validate targets")?;
    Ok(())
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub(crate) struct Config {
    // Apps registry
    pub(crate) apps: Vec<AppConfig>,
    // Meta registry for all targets (e.g. health-check)
    #[serde(default)]
    pub(crate) targets: HashMap<d::Target, TargetConfig>,
}

// Target-specific (app-agnostic) meta
// (e.g. health-checks+healthiness, failure_count)
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub(crate) struct TargetConfig {
    #[serde(default)]
    pub(crate) health_check: HealthCheckStrategyConfig,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(
    rename_all = "PascalCase",
    rename_all_fields = "PascalCase",
    tag = "Type",
    deny_unknown_fields
)]
pub(crate) enum LoadBalancerStrategyConfig {
    #[default]
    RoundRobin,
    Weighted {
        default_weight: u16,
    },
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub(crate) struct AppConfig {
    pub(crate) name: d::AppName,
    pub(crate) ports: HashSet<u16>,
    pub(crate) targets: HashSet<d::Target>,
    #[serde(default)]
    pub(crate) meta: HashMap<d::Target, d::AppTargetMeta>,
    #[serde(default)]
    pub(crate) load_balancer_strategy: LoadBalancerStrategyConfig,
}

#[derive(Debug, serde::Deserialize)]
#[serde(
    tag = "Type",
    rename_all = "PascalCase",
    rename_all_fields = "PascalCase",
    deny_unknown_fields
)]
pub(crate) enum HealthCheckStrategyConfig {
    Passive {
        failure_count_threshold: u32,
        #[serde(with = "humantime_serde")]
        recovery_timeout: Duration,
    },
    TcpConnect {
        #[serde(with = "humantime_serde")]
        timeout: Duration,
        #[serde(with = "humantime_serde")]
        interval: Duration,
        failure_count_threshold: u32,
    },
    TcpSendExpect {
        #[serde(with = "humantime_serde")]
        timeout: Duration,
        #[serde(with = "humantime_serde")]
        interval: Duration,
        failure_count_threshold: u32,
        send: String,
        expect: String,
    },
}

impl Default for HealthCheckStrategyConfig {
    fn default() -> Self {
        // nginx has these defaults: https://docs.nginx.com/nginx/admin-guide/load-balancer/tcp-health-check/#passive-tcp-health-checks
        Self::Passive {
            failure_count_threshold: 1,
            recovery_timeout: Duration::from_secs(10),
        }
    }
}
