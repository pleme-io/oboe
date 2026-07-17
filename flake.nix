{
  description = "oboe — adaptive DNS posture orchestrator (probes upstreams, steers macOS+dnsmasq config based on health)";

  # substrate.rust.workspace dispatches over Cargo.gen.lock (the slim gen delta,
  # reconstructed to the full BuildSpec in pure Nix) — no crate2nix, no Cargo.nix.
  inputs.substrate.url = "github:pleme-io/substrate";

  outputs = { substrate, ... }: substrate.rust.workspace {
    src = ./.;
    member = "oboe";
  };
}
