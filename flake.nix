{
  description = "A development shell with Node.js 22 and node2nix build";

  inputs = {
    nixpkgs.url = "github:meta-introspector/nixpkgs"; # Updated nixpkgs URL
    flake-utils.url = "github:numtide/flake-utils";
    node2nix-src.url = "github:meta-introspector/node2nix";
  };

  outputs = { self, nixpkgs, flake-utils, node2nix-src }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = [
            pkgs.nodejs_22
            node2nix-src.packages.${system}.default
          ];
        };
        packages.node2nix = node2nix-src.packages.${system}.default;
      }
    );
}