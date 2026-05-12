# oboe (覚え)

Adaptive DNS posture orchestrator for pleme-io darwin nodes.

Probes the upstreams listed in `/etc/blackmatter/dns-posture.json` and
rewrites the macOS system resolver chain (`networksetup
-setdnsservers`) whenever an upstream's health crosses the
Healthy/Unhealthy boundary. The dnsmasq layer handles upstream
resilience in parallel (`all-servers`) — oboe handles the *system*
resolver chain that macOS falls through to when dnsmasq itself is
dead or refuses queries.

## Build

```
nix build
./result/bin/oboe --help
```

## Run

```
sudo oboe daemon --posture /etc/blackmatter/dns-posture.json
```

Or via the nix-darwin module:

```nix
services.oboe.enable = true;
```

## CLI

| Command | What |
|---|---|
| `oboe daemon` | Run the probe loop + effector |
| `oboe status` | Print `/var/lib/oboe/state.json` |
| `oboe probe <ip>` | One-shot probe of a single upstream |

## License

MIT
