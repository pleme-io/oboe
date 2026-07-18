{
  description = "oboe — adaptive DNS posture orchestrator (probes upstreams, steers macOS+dnsmasq config based on health)";

  # substrate.rust.workspace dispatches over Cargo.gen.lock (the slim gen delta,
  # reconstructed to the full BuildSpec in pure Nix) — no crate2nix, no Cargo.nix.
  inputs.substrate.url = "github:pleme-io/substrate";

  outputs = { substrate, ... }: substrate.rust.workspace {
    src = ./.;
    member = "oboe";
    # Restore the module-trio spec the bare substrate.rust.workspace shape dropped
    # (43cb04a) — the builder synthesizes homeManagerModules/nixosModules/darwinModules
    # .default AND overlays.default (defaulting services.oboe.package = pkgs.oboe) from
    # this spec. The fleet consumes both: nix/darwinConfigurations (darwinModules) and
    # nix/parts/overlays.nix (overlays.default). Exact pre-conversion spec.
    module = {
      description = "oboe (覚え) — adaptive DNS posture orchestrator";
      hmNamespace = "blackmatter.components";
      withUserDaemon = false; # System-level, not per-user.
      withShikumiConfig = false;
    };
  };
}
