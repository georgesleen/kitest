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
              ngspice
              python3
              uv
              kicad-small
            ];

            # Never let uv fetch its own python; it uses the nix python3 on PATH.
            # (Do not set UV_PYTHON to the store path: maturin develop's internal
            # `uv pip install` would then target the immutable /nix/store and fail.
            # The venv target pins the interpreter per-command instead.)
            env.UV_PYTHON_DOWNLOADS = "never";
          };
        }
      );
    };
}
