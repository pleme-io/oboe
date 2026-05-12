# module/default.nix — nix-darwin LaunchDaemon for oboe.
#
# Reads /etc/blackmatter/dns-posture.json (written by the
# blackmatter.profiles.macos.dns module when `cfg.posture` is set),
# probes each upstream, and rewrites the macOS system resolver chain
# via `networksetup -setdnsservers` whenever an upstream's health
# crosses the Healthy/Unhealthy boundary.
#
# Coexists with blackmatter.profiles.macos.dns's static activation
# write — oboe takes over once it's running, but the static write at
# activation provides the boot-time chain.
{ config, lib, pkgs, ... }:

with lib;

let
  cfg = config.services.oboe;
in
{
  options.services.oboe = {
    enable = mkOption {
      type = types.bool;
      default = false;
      description = ''
        Run oboe as a system LaunchDaemon. Probes the upstream
        resolvers listed in /etc/blackmatter/dns-posture.json and
        steers the macOS system resolver chain based on observed
        health. Pairs with blackmatter.profiles.macos.dns (the
        posture sidecar producer).
      '';
    };

    package = mkOption {
      type = types.package;
      description = "The oboe package to use.";
    };

    posturePath = mkOption {
      type = types.path;
      default = /etc/blackmatter/dns-posture.json;
      description = "Path to the DNS posture JSON sidecar.";
    };

    services = mkOption {
      type = types.listOf types.str;
      default = [ "Wi-Fi" "Thunderbolt Bridge" ];
      description = ''
        macOS network services oboe installs resolver chains into.
        Only enabled services receive updates; the others are skipped
        at apply time.
      '';
    };

    localResolver = mkOption {
      type = types.str;
      default = "127.0.0.1";
      description = ''
        Local resolver address (typically dnsmasq) installed first in
        every chain. macOS queries this first; oboe-managed
        fallbacks follow.
      '';
    };

    logLevel = mkOption {
      type = types.enum [ "trace" "debug" "info" "warn" "error" ];
      default = "info";
      description = "Daemon log filter (`RUST_LOG=oboe=<level>`).";
    };
  };

  config = mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];

    launchd.daemons.oboe = {
      script = ''
        exec ${cfg.package}/bin/oboe daemon \
          --posture ${toString cfg.posturePath} \
          ${lib.concatMapStringsSep " " (s: ''--service "${s}"'') cfg.services} \
          --local-resolver ${cfg.localResolver}
      '';
      serviceConfig = {
        Label = "io.pleme.oboe";
        RunAtLoad = true;
        KeepAlive = true;
        StandardOutPath = "/var/log/oboe.log";
        StandardErrorPath = "/var/log/oboe.log";
        EnvironmentVariables = {
          RUST_LOG = "oboe=${cfg.logLevel}";
          PATH = "/usr/bin:/usr/sbin:/bin:/sbin";
        };
      };
    };

    # Pre-create the state dir oboe writes its snapshot into. /var/lib
    # is the canonical macOS form; oboe writes to /var/lib/oboe/state.json.
    system.activationScripts.oboeStateDir.text = ''
      /usr/bin/install -d -m 0755 -o root -g wheel /var/lib/oboe 2>/dev/null || true
    '';
  };
}
