{
  description = "Gemini CLI with bundled JavaScript and development environment";

  inputs = {
    nixpkgs.url = "github:meta-introspector/nixpkgs";
    flake-utils.url = "github:numtide/flake-utils";
    node2nix-src.url = "github:meta-introspector/node2nix";
  };

  outputs = { self, nixpkgs, flake-utils, node2nix-src }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        
        # Create the gemini-cli package with the existing bundle
        gemini-cli = pkgs.stdenv.mkDerivation {
          pname = "gemini-cli";
          version = "0.8.0-nightly.20250925.b1da8c21";
          
          src = ./.;
          
          buildInputs = [ pkgs.nodejs_22 ];
          
          dontBuild = true;
          
          installPhase = ''
            mkdir -p $out/bin
            mkdir -p $out/share/gemini-cli
            
            echo "=== Checking source contents ==="
            ls -la
            find . -name "*bundle*" -o -name "gemini.js" | head -10
            
            if [ -d bundle ]; then
              echo "✓ Found bundle directory, copying..."
              cp -r bundle $out/share/gemini-cli/
              chmod +x $out/share/gemini-cli/bundle/gemini.js
              
              if [ -f $out/share/gemini-cli/bundle/gemini.js ]; then
                echo "✓ Successfully copied gemini.js ($(stat -c%s $out/share/gemini-cli/bundle/gemini.js) bytes)"
                
                # Create wrapper script
                cat > $out/bin/gemini << EOF
            #!/usr/bin/env bash
            exec ${pkgs.nodejs_22}/bin/node $out/share/gemini-cli/bundle/gemini.js "\$@"
            EOF
                chmod +x $out/bin/gemini
                
                # Create direct symlink
                ln -s $out/share/gemini-cli/bundle/gemini.js $out/bin/gemini.js
                
                echo "✓ Gemini CLI package created successfully"
              else
                echo "✗ Failed to copy gemini.js"
                exit 1
              fi
            else
              echo "✗ No bundle directory found in source"
              echo "Available directories and files:"
              ls -la
              exit 1
            fi
          '';
          
          meta = with pkgs.lib; {
            description = "Google Gemini CLI tool";
            homepage = "https://github.com/google-gemini/gemini-cli";
            license = licenses.asl20;
            maintainers = [ ];
            platforms = platforms.all;
          };
        };
      in
      {
        packages = {
          default = gemini-cli;
          gemini-cli = gemini-cli;
          node2nix = node2nix-src.packages.${system}.default;
        };
        
        devShells.default = pkgs.mkShell {
          buildInputs = [
            pkgs.nodejs_22
            node2nix-src.packages.${system}.default
          ];
        };
        
        apps = {
          default = {
            type = "app";
            program = "${gemini-cli}/bin/gemini";
          };
          gemini = {
            type = "app"; 
            program = "${gemini-cli}/bin/gemini";
          };
        };
      }
    );
}