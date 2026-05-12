//! Per-upstream health state machine. Consumes `ProbeOutcome`s,
//! derives `ServerHealth`, and produces the effective resolver chain
//! the macOS effector should install.

use serde::Serialize;
use std::collections::BTreeMap;
use std::net::IpAddr;

use crate::probe::ProbeOutcome;
use oboe_types::{DnsPosture, FallbackPolicy, ServerHealth};

#[derive(Debug, Clone, Serialize)]
pub struct ServerState {
    pub health: ServerHealth,
    pub consecutive_failures: u32,
    pub consecutive_successes: u32,
    pub last_rtt_ms: Option<u64>,
    pub last_error: Option<String>,
}

impl ServerState {
    const fn unknown() -> Self {
        Self {
            // Assume healthy until proven otherwise — matches what
            // the static config would have set anyway.
            health: ServerHealth::Healthy,
            consecutive_failures: 0,
            consecutive_successes: 0,
            last_rtt_ms: None,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StateSnapshot {
    pub posture_label: String,
    pub posture_id: String,
    pub ordered_upstreams: Vec<IpAddr>,
    pub servers: BTreeMap<IpAddr, ServerState>,
    pub effective_chain: Vec<IpAddr>,
}

pub struct StateMachine {
    posture_label: String,
    posture_id: String,
    /// Original posture order — preserved for `promote-secondary`
    /// fallback (we re-prefix healthy in this order; unhealthy go to
    /// the back).
    order: Vec<IpAddr>,
    servers: BTreeMap<IpAddr, ServerState>,
    failures_before_unhealthy: u32,
    successes_before_healthy: u32,
    fallback: FallbackPolicy,
}

impl StateMachine {
    #[must_use]
    pub fn new(posture: &DnsPosture) -> Self {
        let mut servers = BTreeMap::new();
        for ip in &posture.upstream_servers {
            servers.insert(*ip, ServerState::unknown());
        }
        Self {
            posture_label: posture.label.clone(),
            posture_id: posture.posture_id.clone(),
            order: posture.upstream_servers.clone(),
            servers,
            failures_before_unhealthy: posture.health.failures_before_unhealthy,
            successes_before_healthy: posture.health.successes_before_healthy,
            fallback: posture.fallback.clone(),
        }
    }

    /// Apply a probe outcome to the state machine. Returns `true` if
    /// the effective chain changed and the effector should re-apply.
    pub fn observe(&mut self, outcome: ProbeOutcome) -> bool {
        let entry = self
            .servers
            .entry(outcome.upstream)
            .or_insert_with(ServerState::unknown);
        let prior = entry.health;
        entry.last_rtt_ms = Some(outcome.rtt_ms);
        if outcome.ok {
            entry.consecutive_failures = 0;
            entry.consecutive_successes = entry.consecutive_successes.saturating_add(1);
            entry.last_error = None;
            if entry.consecutive_successes >= self.successes_before_healthy {
                entry.health = ServerHealth::Healthy;
            } else if entry.health == ServerHealth::Unhealthy {
                entry.health = ServerHealth::Probing;
            }
        } else {
            entry.consecutive_successes = 0;
            entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
            entry.last_error = outcome.error;
            if entry.consecutive_failures >= self.failures_before_unhealthy {
                entry.health = ServerHealth::Unhealthy;
            } else {
                entry.health = ServerHealth::Probing;
            }
        }
        let new_membership = matches!(entry.health, ServerHealth::Unhealthy)
            != matches!(prior, ServerHealth::Unhealthy);
        new_membership
    }

    /// The resolver chain the macOS effector should install. Order:
    /// healthy/probing first (preserving posture order), then any
    /// unhealthy as a last-resort tail so a wedged primary doesn't
    /// black-hole DNS.
    #[must_use]
    pub fn effective_chain(&self) -> Vec<IpAddr> {
        match self.fallback {
            FallbackPolicy::PromoteSecondary | FallbackPolicy::SwitchPosture { .. } => {
                let mut head = Vec::new();
                let mut tail = Vec::new();
                for ip in &self.order {
                    let usable = self
                        .servers
                        .get(ip)
                        .map(|s| s.health.is_usable())
                        .unwrap_or(true);
                    if usable {
                        head.push(*ip);
                    } else {
                        tail.push(*ip);
                    }
                }
                head.extend(tail);
                head
            }
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> StateSnapshot {
        StateSnapshot {
            posture_label: self.posture_label.clone(),
            posture_id: self.posture_id.clone(),
            ordered_upstreams: self.order.clone(),
            servers: self.servers.clone(),
            effective_chain: self.effective_chain(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oboe_types::{ExpectKind, FallbackPolicy, HealthConfig};
    use std::collections::BTreeMap;

    fn posture(ips: Vec<&str>) -> DnsPosture {
        DnsPosture {
            label: "test".into(),
            description: String::new(),
            posture_id: "test".into(),
            upstream_servers: ips.into_iter().map(|s| s.parse().unwrap()).collect(),
            addresses: BTreeMap::new(),
            health: HealthConfig {
                query: "example.com".into(),
                expect: ExpectKind::AnswerNonempty,
                interval_ms: 1000,
                timeout_ms: 500,
                failures_before_unhealthy: 3,
                successes_before_healthy: 2,
            },
            fallback: FallbackPolicy::PromoteSecondary,
        }
    }

    fn outcome(ip: &str, ok: bool) -> ProbeOutcome {
        ProbeOutcome {
            upstream: ip.parse().unwrap(),
            ok,
            rtt_ms: 50,
            error: if ok { None } else { Some("test".into()) },
        }
    }

    #[test]
    fn unhealthy_after_threshold() {
        let mut sm = StateMachine::new(&posture(vec!["1.1.1.1", "9.9.9.9"]));
        assert_eq!(sm.effective_chain(), vec!["1.1.1.1".parse::<IpAddr>().unwrap(), "9.9.9.9".parse::<IpAddr>().unwrap()]);
        assert!(!sm.observe(outcome("1.1.1.1", false))); // 1 failure, still Probing → usable, no change
        assert!(!sm.observe(outcome("1.1.1.1", false))); // 2 failures
        assert!(sm.observe(outcome("1.1.1.1", false))); // 3 failures → Unhealthy → chain changed
        // 1.1.1.1 moves to tail
        assert_eq!(
            sm.effective_chain(),
            vec![
                "9.9.9.9".parse::<IpAddr>().unwrap(),
                "1.1.1.1".parse::<IpAddr>().unwrap(),
            ]
        );
    }

    #[test]
    fn recovery_after_successes() {
        let mut sm = StateMachine::new(&posture(vec!["1.1.1.1", "9.9.9.9"]));
        // Take 1.1.1.1 to Unhealthy.
        sm.observe(outcome("1.1.1.1", false));
        sm.observe(outcome("1.1.1.1", false));
        assert!(sm.observe(outcome("1.1.1.1", false)));
        assert_eq!(sm.effective_chain()[0], "9.9.9.9".parse::<IpAddr>().unwrap());
        // One success: Probing (still demoted in chain — not usable until Healthy).
        // Wait — Probing IS usable per ServerHealth::is_usable; so after one
        // success it should already be re-promoted.
        assert!(sm.observe(outcome("1.1.1.1", true)));
        assert_eq!(sm.effective_chain()[0], "1.1.1.1".parse::<IpAddr>().unwrap());
    }
}
