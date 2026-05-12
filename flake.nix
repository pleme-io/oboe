{
  description = "oboe — adaptive DNS posture orchestrator (probes upstreams, steers macOS+dnsmasq config based on health)";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-25.11";
    crate2nix.url = "github:nix-community/crate2nix";
    flake-utils.url = "github:numtide/flake-utils";
    substrate = {
      url = "github:pleme-io/substrate";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    crate2nix,
    flake-utils,
    substrate,
  }:
    (import "${substrate}/lib/rust-workspace-release-flake.nix" {
      inherit nixpkgs crate2nix flake-utils;
    }) {
      toolName = "oboe";
      packageName = "oboe";
      src = self;
      repo = "pleme-io/oboe";

      module = {
        description = "oboe (覚え) — adaptive DNS posture orchestrator";
        hmNamespace = "blackmatter.components";

        # Daemon command surface: `oboe daemon` runs the probe loop.
        withUserDaemon = false; # System-level, not per-user.

        # Shikumi YAML config — minimal for now (probe defaults +
        # service list overrides). Posture file path comes through.
        withShikumiConfig = false;
      };
    };
}
