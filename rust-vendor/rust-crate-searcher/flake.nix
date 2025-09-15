{
  description = "Nix flake for rust-crate-searcher";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    naersk.url = "github:nix-community/naersk/master";
  };

  outputs = { self, nixpkgs, naersk }:
    let
      system = "aarch64-linux"; # Assuming aarch64-linux as the target system
      pkgs = import nixpkgs { inherit system; };
      naerskLib = naersk.lib.${system};
    in
    {
      packages.${system}.default = naerskLib.buildPackage {
        pname = "rust-crate-searcher";
        version = "0.1.0";

        src = ./.; # Source is the current directory (rust-crate-searcher)

        # naersk will generate Cargo.lock if it's not present, or verify it.
        # We can start with a dummy hash if Cargo.lock is not present.
        # cargoLock = {
        #   lockFile = ./Cargo.lock;
        # };

        # Optional: if you need specific build inputs for the Rust project
        # buildInputs = with pkgs; [
        #   # Add any system dependencies needed for the Rust project
        # ];

        # Optional: if you need to run tests
        # doCheck = true;

        meta = with pkgs.lib; {
          description = "A conceptual Rust crate searcher";
          homepage = "https://github.com/example/rust-crate-searcher"; # Placeholder
          license = licenses.mit; # Placeholder
          platforms = platforms.all;
        };
      };
    };
}
