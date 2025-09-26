{
  description = "Development shell for gemini-cli source code";

  inputs = {
    nixpkgs.url = "github:meta-introspector/nixpkgs?ref=feature/CRQ-016-nixify";
    node2nix.url = "github:meta-introspector/node2nix?ref=feature/gemini-cli";
  };

  outputs = { self, nixpkgs, ... }:
    let
      systems = [ "aarch64-linux" "x86_64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        import ./nix/packages.nix { inherit pkgs self; }
      );

      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          default = import ./nix/shell.nix { inherit pkgs; };
        });
    };
}