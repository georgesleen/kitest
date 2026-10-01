{
  description = "kitest dev shell";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { nixpkgs, ... }:
    let
      forAllSystems = nixpkgs.lib.genAttrs nixpkgs.lib.systems.flakeExposed;
    in
    {
      devShells = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          pythonEnv = pkgs.python3.withPackages (ps: [ ps.matplotlib ]);
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              git
              gnumake
              nixfmt
              rustc
              cargo
              clippy
              rustfmt
              rust-analyzer
              # lldb-dap: the DAP adapter helix drives for `:debug-start`.
              lldb
              ngspice
              pythonEnv
              uv
              kicad-small
            ];

            # Never let uv fetch its own python; it uses the nix python3 on PATH.
            # (Do not set UV_PYTHON to the store path: maturin develop's internal
            # `uv pip install` would then target the immutable /nix/store and fail.
            # The venv target pins the interpreter per-command instead.)
            env.UV_PYTHON_DOWNLOADS = "never";

            # matplotlib backs kitest.scope. Taken from nixpkgs, where its
            # native libraries already resolve, and put on PYTHONPATH so the
            # uv venv sees it without a binary wheel.
            env.PYTHONPATH = "${pythonEnv}/${pkgs.python3.sitePackages}";
          };
        }
      );
    };
}
