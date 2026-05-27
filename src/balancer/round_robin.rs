use anyhow::anyhow;

use crate::balancer::Balancer;
use crate::{config as cfg, domain as d};

/// A simple round robin balancer.
#[derive(Debug)]
pub struct RoundRobinBalancer {
    next_index: usize,
}

impl Balancer for RoundRobinBalancer {
    fn route<'active>(
        &mut self,
        _app_targets_meta: &std::collections::HashMap<d::Target, d::AppTargetMeta>,
        active_targets: Vec<&'active d::Target>,
    ) -> Option<&'active d::Target> {
        if active_targets.is_empty() {
            return None;
        }
        while self.next_index >= active_targets.len() {
            self.next_index -= active_targets.len();
        }
        debug_assert!(self.next_index < active_targets.len());
        let use_index = self.next_index;
        self.next_index += 1;
        Some(active_targets[use_index])
    }
}

impl TryFrom<&cfg::LoadBalancerStrategyConfig> for RoundRobinBalancer {
    type Error = anyhow::Error;
    fn try_from(config: &cfg::LoadBalancerStrategyConfig) -> Result<Self, Self::Error> {
        match config {
            cfg::LoadBalancerStrategyConfig::RoundRobin => Ok(Self { next_index: 0 }),
            _ => Err(anyhow!(
                "RoundRobinBalancer cannot be constructed by given LoadBalancerStrategyConfig"
            )),
        }
    }
}
