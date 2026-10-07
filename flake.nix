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
          # Every Python dependency comes from nixpkgs: matplotlib backs
          # kitest.scope, pytest runs the tests, mkdocs builds the docs site,
          # maturin builds release wheels.
          pythonEnv = pkgs.python3.withPackages (ps: [
            ps.matplotlib
            ps.pytest
            ps.mkdocs
            ps.mkdocs-material
            ps.mkdocstrings
            ps.mkdocstrings-python
          ]);
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
              maturin
              kicad-small
            ];

          };
        }
      );
    };
}
