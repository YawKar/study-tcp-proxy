use std::sync::LazyLock;

use anyhow::Context;
use dashmap::DashMap;
use nutype::nutype;
use regex::Regex;

use crate::health;

#[nutype(
    validate(with = validate_target, error = anyhow::Error),
    derive(AsRef, Clone, Debug, Deserialize, Eq, Hash, PartialEq),
)]
pub(crate) struct Target(String);

static TARGET_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[a-zA-Z0-9][a-zA-Z0-9.-]*\.?:([0-9]{1,5})$")
        .expect("target validation regex should compile successfully")
});

fn validate_target(endpoint: &str) -> anyhow::Result<()> {
    TARGET_RE
        .captures(endpoint)
        .with_context(|| {
            let re = TARGET_RE.as_str();
            format!("given target doesn't satisfy the following regex: {re}")
        })?
        .get(1)
        .with_context(|| "failed to get port group match during target validation")?
        .as_str()
        .parse::<u16>()
        .with_context(|| "given port is not in the [0; 65535] interval")?;
    Ok(())
}

#[nutype(
    validate(not_empty, regex = r"^[a-zA-Z0-9-_]+$"),
    derive(PartialEq, Eq, Hash, Clone, Debug, Deserialize)
)]
pub(crate) struct AppName(String);

// App-specific endpoint meta (lb weight)
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub(crate) struct AppTargetMeta {
    pub(crate) load_balancer_strategy_meta: Option<LoadBalancerStrategyTargetMeta>,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase", tag = "Type", deny_unknown_fields)]
pub(crate) enum LoadBalancerStrategyTargetMeta {
    RoundRobin,
    Weighted { weight: u16 },
}

pub(crate) type TargetRegistry = DashMap<Target, TargetState>;
// AppTarget runtime-specific state. (i.e. healthiness)
pub(crate) struct TargetState {
    // 1 Target <-> 1 HealthCheck
    pub(crate) health_check: health::HealthCheck,
}
