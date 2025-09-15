{
  description = "Development shell for gemini-cli source code";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-23.11"; # Pin to a stable NixOS release
  };

  outputs = { self, nixpkgs }:
    let
      system = "aarch64-linux"; # Assuming x86_64-linux, adjust if needed
      pkgs = import nixpkgs { inherit system; };

      geminiWrapper = geminiCliOutput: pkgs.writeShellApplication {
        name = "gemini";
        runtimeInputs = [ pkgs.nodejs_21 ];
        text = ''
          exec ${pkgs.nodejs_21}/bin/node ${geminiCliOutput}/bin/gemini-cli-dist/index.js "$@"
        '';
      };
    in
    {
      packages.${system}.default = pkgs.stdenv.mkDerivation rec {
        pname = "gemini-cli";
        version = "0.6.0-nightly"; # You might want to get this from package.json

        src = self;

        nativeBuildInputs = [ pkgs.nodejs_21 pkgs.git ];

        installPhase = ''
          # Due to persistent memory corruption issues with npm in the Nix build sandbox,
          # we are assuming the project is pre-built outside of Nix.
          # The 'packages/cli/dist' directory is expected to exist in the source.

          mkdir -p $out/bin
          cp -r packages/cli/dist $out/bin/gemini-cli-dist

          # Install the gemini wrapper
          cp ${ (geminiWrapper pkgs.stdenv.outPath) }/bin/gemini $out/bin/gemini
        '';
      };

      devShells.${system}.default = pkgs.mkShell {
        buildInputs = [ pkgs.nodejs_21 pkgs.which pkgs.git ];
        # Add any other development tools needed for gemini-cli here
      };
    };
}