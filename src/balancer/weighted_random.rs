use anyhow::anyhow;

use crate::balancer::Balancer;
use crate::{config as cfg, domain as d};

#[derive(Debug)]
pub struct WeightedBalancer {
    default_weight: u16,
}

impl Balancer for WeightedBalancer {
    fn route<'active>(
        &mut self,
        app_targets_meta: &std::collections::HashMap<d::Target, d::AppTargetMeta>,
        active_targets: Vec<&'active d::Target>,
    ) -> Option<&'active d::Target> {
        let weighted_targets: Vec<(&d::Target, u32)> = active_targets
            .iter()
            .copied()
            .zip(active_targets.iter().map(|target| {
                app_targets_meta
                    .get(target)
                    .map_or(self.default_weight, |meta| {
                        if let Some(d::LoadBalancerStrategyTargetMeta::Weighted { weight }) =
                            meta.load_balancer_strategy_meta
                        {
                            weight
                        } else {
                            self.default_weight
                        }
                    })
                    .into()
            }))
            .filter(|&(_, w)| w > 0)
            .collect();
        if weighted_targets.is_empty() {
            // case when user set all targets' weights to 0
            return None;
        }
        let total_weight: u32 = weighted_targets.iter().map(|&(_, w)| w).sum();
        debug_assert!(total_weight > 0);
        debug_assert!(!weighted_targets.is_empty());
        let shift = fastrand::u32(0..total_weight);
        let mut cumulative_sum = 0;
        for (target, weight) in weighted_targets {
            cumulative_sum += weight;
            if cumulative_sum > shift {
                return Some(target);
            }
        }
        unreachable!(
            "it should be impossible not to choose at least 1 variant with given preconditions"
        )
    }
}

impl TryFrom<&cfg::LoadBalancerStrategyConfig> for WeightedBalancer {
    type Error = anyhow::Error;
    fn try_from(config: &cfg::LoadBalancerStrategyConfig) -> Result<Self, Self::Error> {
        match config {
            cfg::LoadBalancerStrategyConfig::Weighted { default_weight } => Ok(Self {
                default_weight: *default_weight,
            }),
            _ => Err(anyhow!(
                "WeightedBalancer cannot be constructed by given LoadBalancerStrategyConfig"
            )),
        }
    }
}
