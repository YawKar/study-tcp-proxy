mod round_robin;
mod weighted_random;

use std::collections::HashMap;

use round_robin::RoundRobinBalancer;
use weighted_random::WeightedBalancer;

use crate::{config as cfg, domain as d};

#[derive(Debug)]
pub(crate) enum LoadBalancerStrategy {
    RoundRobin(RoundRobinBalancer),
    Weighted(WeightedBalancer),
}

trait Balancer {
    /// The single important method.
    ///
    /// Should select a single target from the given `active_targets`.
    /// For any target-specific meta data it should query the given app_targets_meta.
    /// `active_targets` is not guaranteed to have at least 1 active target, in this case it should return `None`.
    fn route<'active>(
        &mut self,
        app_targets_meta: &HashMap<d::Target, d::AppTargetMeta>,
        active_targets: Vec<&'active d::Target>,
    ) -> Option<&'active d::Target>;
}

impl LoadBalancerStrategy {
    pub(crate) fn route<'active>(
        &mut self,
        app_targets_meta: &HashMap<d::Target, d::AppTargetMeta>,
        active_targets: Vec<&'active d::Target>,
    ) -> Option<&'active d::Target> {
        if active_targets.is_empty() {
            return None;
        }
        match self {
            Self::RoundRobin(balancer) => balancer.route(app_targets_meta, active_targets),
            Self::Weighted(balancer) => balancer.route(app_targets_meta, active_targets),
        }
    }
}

impl TryFrom<&cfg::LoadBalancerStrategyConfig> for LoadBalancerStrategy {
    type Error = anyhow::Error;
    fn try_from(config: &cfg::LoadBalancerStrategyConfig) -> Result<Self, Self::Error> {
        match config {
            cfg::LoadBalancerStrategyConfig::RoundRobin => {
                RoundRobinBalancer::try_from(config).map(Self::RoundRobin)
            }
            cfg::LoadBalancerStrategyConfig::Weighted { .. } => {
                WeightedBalancer::try_from(config).map(Self::Weighted)
            }
        }
    }
}
