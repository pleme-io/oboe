//! macOS effector — installs a resolver chain into each enabled
//! network service via `/usr/sbin/networksetup -setdnsservers`.
//!
//! The local resolver (dnsmasq, typically `127.0.0.1`) is always the
//! first entry; remaining entries come from the state machine's
//! effective chain (healthy upstreams first, unhealthy as last resort
//! tail).

use anyhow::{Context, Result};
use std::net::IpAddr;
use std::process::Command;
use tracing::{info, warn};

pub struct MacosEffector {
    services: Vec<String>,
    local_resolver: IpAddr,
}

impl MacosEffector {
    #[must_use]
    pub const fn new(services: Vec<String>, local_resolver: IpAddr) -> Self {
        Self {
            services,
            local_resolver,
        }
    }

    /// Apply the chain to every enabled network service.
    ///
    /// # Errors
    ///
    /// Returns the first error encountered when invoking
    /// `networksetup`; subsequent services are still attempted.
    pub fn apply(&self, chain: &[IpAddr]) -> Result<()> {
        let mut full_chain = vec![self.local_resolver];
        full_chain.extend_from_slice(chain);
        let chain_strs: Vec<String> = full_chain.iter().map(ToString::to_string).collect();

        let mut first_error: Option<anyhow::Error> = None;
        for service in &self.services {
            if !service_enabled(service) {
                continue;
            }
            match set_dns_servers(service, &chain_strs) {
                Ok(()) => info!(service = %service, chain = ?chain_strs, "installed resolver chain"),
                Err(e) => {
                    warn!(service = %service, error = %e, "networksetup failed");
                    if first_error.is_none() {
                        first_error = Some(e);
                    }
                }
            }
        }

        // Flush so the new chain is picked up immediately.
        let _ = Command::new("/usr/bin/dscacheutil").arg("-flushcache").status();
        let _ = Command::new("/usr/bin/killall").args(["-HUP", "mDNSResponder"]).status();

        if let Some(e) = first_error {
            Err(e)
        } else {
            Ok(())
        }
    }
}

fn service_enabled(service: &str) -> bool {
    Command::new("/usr/sbin/networksetup")
        .args(["-getnetworkserviceenabled", service])
        .output()
        .map(|out| {
            String::from_utf8_lossy(&out.stdout)
                .trim()
                .eq_ignore_ascii_case("enabled")
        })
        .unwrap_or(false)
}

fn set_dns_servers(service: &str, servers: &[String]) -> Result<()> {
    let mut cmd = Command::new("/usr/sbin/networksetup");
    cmd.arg("-setdnsservers").arg(service);
    for s in servers {
        cmd.arg(s);
    }
    let status = cmd
        .status()
        .with_context(|| format!("invoking networksetup for service {service}"))?;
    if !status.success() {
        anyhow::bail!("networksetup exited with status {status}");
    }
    Ok(())
}
