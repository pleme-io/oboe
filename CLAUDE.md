# oboe — Claude Orientation

One-sentence purpose: adaptive DNS posture orchestrator. Probes the
upstreams declared in `/etc/blackmatter/dns-posture.json`, tracks
per-upstream health, and rewrites the macOS system resolver chain
(`networksetup -setdnsservers`) so the kernel resolver never serially
walks a known-dead server.

## Architecture

Two layers:

- **dnsmasq** (data plane): answers queries, fans every upstream
  query in parallel via `all-servers`. Always-on; nothing oboe
  changes here in the MVP.
- **macOS system resolver** (kernel/libc serial walk): falls through
  to oboe's chain when dnsmasq is dead or refuses. Oboe demotes
  unhealthy entries to the tail so a fallback never blocks behind a
  dead one.

## Where to look

| Intent | File |
|--------|------|
| Posture schema (file on disk) | `crates/oboe-types/src/lib.rs` |
| Probe engine | `crates/oboe/src/probe.rs` |
| Health state machine | `crates/oboe/src/state.rs` |
| macOS effector | `crates/oboe/src/effector.rs` |
| CLI surface | `crates/oboe/src/cli.rs` |
| nix-darwin LaunchDaemon module | `module/default.nix` |
| Substrate flake | `flake.nix` (rust-workspace-release-flake) |

## Posture file producer

The blackmatter macOS dns module writes the posture sidecar:

```
blackmatter.profiles.macos.dns.posture = {
  label = "baseline";
  postureId = "...";
  upstreamServers = [ "1.1.1.1" "9.9.9.9" ... ];
  health = { query = "cloudflare.com"; expect = "answer-nonempty"; intervalMs = 5000; ... };
  fallback = { kind = "promote-secondary"; };
};
```

When set, the module renders the JSON to `/etc/blackmatter/dns-posture.json`.
Without it, oboe has nothing to read and the daemon won't start.

## MVP scope (current)

- Probe loop per upstream (`hickory-resolver`, UDP/53)
- `Healthy` / `Probing` / `Unhealthy` per upstream
- Chain reordering: healthy/probing first, unhealthy to tail
- macOS effector: `networksetup -setdnsservers` per service
- One posture at a time (no `switch-posture` runtime swap yet)

## Out of scope (parked)

- Captive-portal detection
- VPN-aware posture switching (auto-promote `vpn-active` when WireGuard is up)
- DoH/DoT probing
- dnsmasq config rewrite (`all-servers` already handles upstream health)
- Cross-host fleet observability
