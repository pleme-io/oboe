//! CLI surface — `oboe daemon`, `oboe status`, `oboe probe <ip>`.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::net::IpAddr;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{info, warn};

use crate::effector::MacosEffector;
use crate::probe::ProbeEngine;
use crate::state::StateMachine;
use oboe_types::DnsPosture;

const DEFAULT_POSTURE_PATH: &str = "/etc/blackmatter/dns-posture.json";

#[derive(Parser, Debug)]
#[command(name = "oboe", version, about = "adaptive DNS posture orchestrator")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run the daemon — probe upstreams continuously, steer the
    /// macOS system resolver chain on state change.
    Daemon {
        #[arg(long, default_value = DEFAULT_POSTURE_PATH)]
        posture: PathBuf,

        /// Network service names to install resolver chains into,
        /// e.g. `--service Wi-Fi --service "Thunderbolt Bridge"`.
        #[arg(long = "service", default_values_t = vec!["Wi-Fi".to_string(), "Thunderbolt Bridge".to_string()])]
        services: Vec<String>,

        /// Local resolver (dnsmasq) installed first in every chain.
        /// macOS queries this first; oboe-managed fallbacks come
        /// after.
        #[arg(long, default_value = "127.0.0.1")]
        local_resolver: IpAddr,
    },
    /// Print the current health state of each upstream as JSON.
    /// (Reads `/var/lib/oboe/state.json` written by the daemon.)
    Status {
        #[arg(long, default_value = "/var/lib/oboe/state.json")]
        state: PathBuf,
    },
    /// One-shot probe of a single upstream — diagnostic.
    Probe {
        upstream: IpAddr,
        #[arg(long, default_value = "cloudflare.com")]
        query: String,
        #[arg(long, default_value_t = 2000)]
        timeout_ms: u64,
    },
}

pub async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Daemon {
            posture,
            services,
            local_resolver,
        } => daemon(&posture, &services, local_resolver).await,
        Command::Status { state } => status(&state),
        Command::Probe {
            upstream,
            query,
            timeout_ms,
        } => probe_one(upstream, &query, timeout_ms).await,
    }
}

async fn daemon(posture_path: &std::path::Path, services: &[String], local: IpAddr) -> Result<()> {
    let posture = DnsPosture::load_from_file(posture_path)
        .with_context(|| format!("loading posture from {posture_path:?}"))?;
    info!(
        label = %posture.label,
        id = %posture.posture_id,
        upstreams = posture.upstream_servers.len(),
        "loaded posture"
    );

    let mut state = StateMachine::new(&posture);
    let engine = ProbeEngine::new(posture.health.clone(), posture.upstream_servers.clone());
    let effector = MacosEffector::new(services.to_vec(), local);

    // Initial chain: assume all healthy until proven otherwise — same
    // as the static config that's already there.
    if let Err(e) = effector.apply(&state.effective_chain()) {
        warn!(error = %e, "initial effector apply failed");
    }

    let mut rx = engine.spawn();
    let state_path = std::path::PathBuf::from("/var/lib/oboe/state.json");

    while let Some(result) = rx.recv().await {
        let changed = state.observe(result);
        if let Err(e) = write_state_snapshot(&state, &state_path) {
            warn!(error = %e, "failed to persist state snapshot");
        }
        if changed {
            let chain = state.effective_chain();
            info!(chain = ?chain, "chain changed, applying");
            if let Err(e) = effector.apply(&chain) {
                warn!(error = %e, "effector apply failed");
            }
        }
    }

    Ok(())
}

fn status(state_path: &std::path::Path) -> Result<()> {
    let raw = std::fs::read_to_string(state_path)
        .with_context(|| format!("reading state from {state_path:?}"))?;
    println!("{raw}");
    Ok(())
}

async fn probe_one(upstream: IpAddr, query: &str, timeout_ms: u64) -> Result<()> {
    let outcome = crate::probe::one_shot(upstream, query, Duration::from_millis(timeout_ms)).await;
    println!(
        "{}",
        serde_json::to_string_pretty(&outcome).expect("serialise probe outcome")
    );
    Ok(())
}

fn write_state_snapshot(state: &StateMachine, path: &std::path::Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let snapshot = state.snapshot();
    let json = serde_json::to_string_pretty(&snapshot)?;
    std::fs::write(path, json)?;
    Ok(())
}
