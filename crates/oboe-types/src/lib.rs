//! Typed primitives for the oboe adaptive DNS posture orchestrator.
//!
//! The blackmatter macOS dns module renders a posture sidecar to
//! `/etc/blackmatter/dns-posture.json` when `cfg.posture != null`.
//! This crate is the source-of-truth deserialiser for that file.
//! Schema is shared with `arch-synthesizer/src/dns_posture/` (renderer
//! side).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::IpAddr;

/// Top-level posture file at `/etc/blackmatter/dns-posture.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsPosture {
    pub label: String,
    #[serde(default)]
    pub description: String,
    #[serde(rename = "postureId")]
    pub posture_id: String,
    #[serde(rename = "upstreamServers")]
    pub upstream_servers: Vec<IpAddr>,
    #[serde(default)]
    pub addresses: BTreeMap<String, IpAddr>,
    pub health: HealthConfig,
    pub fallback: FallbackPolicy,
}

/// Probe configuration the orchestrator uses to decide health.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthConfig {
    /// FQDN to query. e.g. "cloudflare.com".
    pub query: String,
    /// What constitutes a healthy answer.
    pub expect: ExpectKind,
    /// Probe period.
    #[serde(rename = "intervalMs")]
    pub interval_ms: u64,
    /// Per-probe timeout.
    #[serde(rename = "timeoutMs")]
    pub timeout_ms: u64,
    /// Consecutive failures before flipping to Unhealthy.
    #[serde(rename = "failuresBeforeUnhealthy")]
    pub failures_before_unhealthy: u32,
    /// Consecutive successes before flipping back to Healthy.
    /// Optional in the schema; defaults to 2.
    #[serde(rename = "successesBeforeHealthy", default = "default_successes")]
    pub successes_before_healthy: u32,
}

const fn default_successes() -> u32 {
    2
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ExpectKind {
    /// At least one answer record present.
    AnswerNonempty,
    /// DNS response code is `NoError`.
    RcodeOk,
}

/// Action taken when the primary chain is fully Unhealthy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum FallbackPolicy {
    /// Rotate Unhealthy entries to the back of the resolver list so
    /// the macOS serial walk skips them. dnsmasq is unaffected
    /// (`all-servers` already handles upstream health in parallel).
    PromoteSecondary,
    /// Switch to a different authored posture by label. The new
    /// posture file must already exist on disk
    /// (`/etc/blackmatter/dns-postures/<label>.json`).
    SwitchPosture { label: String },
}

/// Per-server probe state.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ServerHealth {
    /// Probes passing.
    Healthy,
    /// One or more consecutive failures, but not yet at threshold.
    Probing,
    /// Failures past `failures_before_unhealthy`. Skipped in the
    /// effective resolver chain.
    Unhealthy,
}

impl ServerHealth {
    #[must_use]
    pub const fn is_healthy(self) -> bool {
        matches!(self, Self::Healthy)
    }

    #[must_use]
    pub const fn is_usable(self) -> bool {
        matches!(self, Self::Healthy | Self::Probing)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("failed to read posture file: {0}")]
    Read(#[from] std::io::Error),
    #[error("failed to parse posture JSON: {0}")]
    Parse(#[from] serde_json::Error),
}

impl DnsPosture {
    /// Load a posture from a JSON file on disk.
    ///
    /// # Errors
    ///
    /// Returns `LoadError::Read` if the file cannot be read, or
    /// `LoadError::Parse` if the contents aren't valid JSON matching
    /// the schema.
    pub fn load_from_file(path: &std::path::Path) -> Result<Self, LoadError> {
        let raw = std::fs::read_to_string(path)?;
        let parsed = serde_json::from_str(&raw)?;
        Ok(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_posture() {
        let json = r#"{
            "label": "baseline",
            "description": "Default chain",
            "postureId": "abc123",
            "upstreamServers": ["1.1.1.1", "9.9.9.9", "8.8.8.8"],
            "addresses": {},
            "health": {
                "query": "cloudflare.com",
                "expect": "answer-nonempty",
                "intervalMs": 5000,
                "timeoutMs": 2000,
                "failuresBeforeUnhealthy": 3
            },
            "fallback": { "kind": "promote-secondary" }
        }"#;
        let posture: DnsPosture = serde_json::from_str(json).unwrap();
        assert_eq!(posture.label, "baseline");
        assert_eq!(posture.upstream_servers.len(), 3);
        assert_eq!(posture.health.successes_before_healthy, 2);
        assert!(matches!(posture.fallback, FallbackPolicy::PromoteSecondary));
    }

    #[test]
    fn parses_switch_posture_fallback() {
        let json = r#"{ "kind": "switch-posture", "label": "vpn-active" }"#;
        let fb: FallbackPolicy = serde_json::from_str(json).unwrap();
        match fb {
            FallbackPolicy::SwitchPosture { label } => assert_eq!(label, "vpn-active"),
            FallbackPolicy::PromoteSecondary => panic!("wrong variant"),
        }
    }
}
