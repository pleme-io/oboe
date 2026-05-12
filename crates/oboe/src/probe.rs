//! DNS probe engine. One async task per upstream; each task sends a
//! configured query on the configured interval and reports outcomes
//! to a channel the state machine consumes.

use anyhow::Result;
use hickory_resolver::config::{NameServerConfig, Protocol, ResolverConfig, ResolverOpts};
use hickory_resolver::TokioAsyncResolver;
use serde::Serialize;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::{debug, instrument};

use oboe_types::{ExpectKind, HealthConfig};

#[derive(Debug, Clone, Serialize)]
pub struct ProbeOutcome {
    pub upstream: IpAddr,
    pub ok: bool,
    pub rtt_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub struct ProbeEngine {
    health: HealthConfig,
    upstreams: Vec<IpAddr>,
}

impl ProbeEngine {
    #[must_use]
    pub const fn new(health: HealthConfig, upstreams: Vec<IpAddr>) -> Self {
        Self { health, upstreams }
    }

    /// Spawn one probe task per upstream. The returned receiver
    /// yields every probe outcome from any upstream.
    pub fn spawn(self) -> mpsc::Receiver<ProbeOutcome> {
        let (tx, rx) = mpsc::channel(128);
        for upstream in self.upstreams {
            let health = self.health.clone();
            let tx = tx.clone();
            tokio::spawn(async move { probe_loop(upstream, health, tx).await });
        }
        rx
    }
}

#[instrument(level = "debug", skip(health, tx), fields(upstream = %upstream))]
async fn probe_loop(upstream: IpAddr, health: HealthConfig, tx: mpsc::Sender<ProbeOutcome>) {
    let interval = Duration::from_millis(health.interval_ms);
    let timeout = Duration::from_millis(health.timeout_ms);
    let mut ticker = tokio::time::interval(interval);
    // Skip the immediate first tick to spread initial load.
    ticker.tick().await;

    loop {
        ticker.tick().await;
        let outcome = single_probe(upstream, &health.query, health.expect, timeout).await;
        debug!(ok = outcome.ok, rtt_ms = outcome.rtt_ms, "probe");
        if tx.send(outcome).await.is_err() {
            // Receiver gone — daemon shutting down.
            break;
        }
    }
}

async fn single_probe(
    upstream: IpAddr,
    query: &str,
    expect: ExpectKind,
    timeout: Duration,
) -> ProbeOutcome {
    let started = Instant::now();
    match tokio::time::timeout(timeout, do_query(upstream, query, expect)).await {
        Ok(Ok(())) => ProbeOutcome {
            upstream,
            ok: true,
            rtt_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            error: None,
        },
        Ok(Err(e)) => ProbeOutcome {
            upstream,
            ok: false,
            rtt_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            error: Some(e.to_string()),
        },
        Err(_) => ProbeOutcome {
            upstream,
            ok: false,
            rtt_ms: u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
            error: Some("timeout".into()),
        },
    }
}

async fn do_query(upstream: IpAddr, query: &str, expect: ExpectKind) -> Result<()> {
    let mut config = ResolverConfig::new();
    config.add_name_server(NameServerConfig {
        socket_addr: SocketAddr::new(upstream, 53),
        protocol: Protocol::Udp,
        tls_dns_name: None,
        trust_negative_responses: true,
        bind_addr: None,
    });
    let mut opts = ResolverOpts::default();
    // Don't let hickory's own retries hide upstream death.
    opts.attempts = 1;
    let resolver = TokioAsyncResolver::tokio(config, opts);
    let lookup = resolver.lookup_ip(query).await?;
    match expect {
        ExpectKind::AnswerNonempty => {
            if lookup.iter().next().is_none() {
                anyhow::bail!("empty answer");
            }
        }
        ExpectKind::RcodeOk => {
            // hickory returns Err on non-NoError rcode; reaching here
            // means NoError. Empty answers still count as RcodeOk.
        }
    }
    Ok(())
}

/// One-shot probe for the `oboe probe <ip>` CLI subcommand.
pub async fn one_shot(upstream: IpAddr, query: &str, timeout: Duration) -> ProbeOutcome {
    single_probe(upstream, query, ExpectKind::AnswerNonempty, timeout).await
}
