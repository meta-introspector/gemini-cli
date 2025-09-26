
{ pkgs, ... }:

let
  # A minimal package.json for node2nix
  packageJson = pkgs.writeText "package.json" ''
    {
      "name": "test-package",
      "version": "1.0.0",
      "dependencies": {
        "lodash": "^4.17.21"
      }
    }
  '';

  # A derivation to run node2nix and capture its output
  node2nixTest = pkgs.stdenv.mkDerivation {
    pname = "node2nix-test";
    version = "1.0";

    src = ./.; # Use the current directory as source

    nativeBuildInputs = [ pkgs.node2nix pkgs.nodejs ];

    buildPhase = ''
      # Create a temporary directory for node2nix output
      mkdir -p $out/output

      # Copy the minimal package.json
      cp ${packageJson} package.json

      # Run node2nix and capture stderr and stdout
      if ! node2nix -i package.json -o $out/output/node-packages.nix -c $out/output/default.nix 2> $out/output/stderr.log > $out/output/stdout.log; then
        echo "node2nix failed with exit code $?" >> $out/output/stderr.log
        exit 1
      fi
    '';

    installPhase = ''
      # Nothing to install, just ensure buildPhase runs
      true
    '';
  };
in
  node2nixTest
