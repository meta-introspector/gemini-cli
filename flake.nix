{
  description = "Development shell for gemini-cli source code";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-23.11"; # Pin to a stable NixOS release
  };

  outputs = { self, nixpkgs }:
    let
      system = "aarch64-linux"; # Assuming x86_64-linux, adjust if needed
      pkgs = import nixpkgs { inherit system; };
    in
    {
      devShells.${system}.default = pkgs.mkShell {
        buildInputs = [ pkgs.nodejs_22 ];
        # Add any other development tools needed for gemini-cli here
      };
    };
}