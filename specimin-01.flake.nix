{
  description = "Gemini CLI with bundled JavaScript and development environment";

  inputs = {
    nixpkgs.url = "github:meta-introspector/nixpkgs"; # Updated nixpkgs URL
    flake-utils.url = "github:numtide/flake-utils";
    node2nix-src.url = "github:meta-introspector/node2nix";
  };

  outputs = { self, nixpkgs, flake-utils, node2nix-src }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        
{
  description = "Gemini CLI with bundled JavaScript and development environment";

  inputs = {
    nixpkgs.url = "github:meta-introspector/nixpkgs"; # Updated nixpkgs URL
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
          
          # Use a filtered source that explicitly includes bundle directory
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              let
                baseName = baseNameOf path;
                relativePath = pkgs.lib.removePrefix (toString ./.) path;
              in
              # Always include bundle directory and its contents
              (pkgs.lib.hasPrefix "/bundle" relativePath) ||
              # Include essential files
              (baseName == "package.json") ||
              (baseName == "package-lock.json") ||
              (baseName == "flake.nix") ||
              (baseName == "flake.lock") ||
              # Include directories we might need
              (type == "directory" && (baseName == "scripts" || baseName == "packages"));
          };
          
          buildInputs = [ pkgs.nodejs_22 ];
          
          # Don't build - use existing bundle
          dontBuild = true;
          
          installPhase = ''
            mkdir -p $out/bin
            mkdir -p $out/share/gemini-cli
            
            echo "Source contents:"
            find . -name "*bundle*" -o -name "gemini.js" | head -10
            
            # Check if bundle directory exists in source
            if [ -d bundle ]; then
              echo "Found bundle directory, copying..."
              cp -r bundle $out/share/gemini-cli/
              chmod +x $out/share/gemini-cli/bundle/gemini.js
              
              # Verify the copy worked
              if [ -f $out/share/gemini-cli/bundle/gemini.js ]; then
                echo "Successfully copied gemini.js ($(stat -c%s $out/share/gemini-cli/bundle/gemini.js) bytes)"
                
                # Create wrapper script that calls the bundled version with node
                cat > $out/bin/gemini << EOF
            #!/usr/bin/env bash
            exec ${pkgs.nodejs_22}/bin/node $out/share/gemini-cli/bundle/gemini.js "\$@"
            EOF
                chmod +x $out/bin/gemini
                
                # Also create direct symlink for advanced usage
                ln -s $out/share/gemini-cli/bundle/gemini.js $out/bin/gemini.js
                
                echo "✓ Gemini CLI package created successfully"
              else
                echo "✗ Failed to copy gemini.js"
                exit 1
              fi
            else
              echo "No bundle directory found in source"
              echo "Available directories:"
              ls -la
              echo "Creating placeholder package..."
              echo "Bundle not available - run 'npm run bundle' first" > $out/share/gemini-cli/README.txt
              
              # Create a script that tells user to build bundle
              cat > $out/bin/gemini << EOF
            #!/usr/bin/env bash
            echo "ERROR: Gemini bundle not found in derivation."
            echo "The bundle directory was not included in the Nix store."
            echo "Available files in derivation:"
            find /nix/store -name "*gemini*" 2>/dev/null | head -5
            exit 1
            EOF
              chmod +x $out/bin/gemini
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
        
        # Alternative package that builds the bundle using npm
        gemini-cli-with-build = pkgs.stdenv.mkDerivation {
          pname = "gemini-cli-with-build";
          version = "0.8.0-nightly.20250925.b1da8c21";
          
          src = ./.;
          
          buildInputs = [ pkgs.nodejs_22 ];
          nativeBuildInputs = [ pkgs.nodejs_22 ];
          
          buildPhase = ''
            echo "Installing npm dependencies..."
            npm ci --prefer-offline --no-audit --progress=false
            
            echo "Running bundle command..."
            npm run bundle
          '';
          
          installPhase = ''
            mkdir -p $out/bin
            mkdir -p $out/share/gemini-cli
            
            # Copy the generated bundle
            cp -r bundle $out/share/gemini-cli/
            chmod +x $out/share/gemini-cli/bundle/gemini.js
            
            # Create wrapper script
            cat > $out/bin/gemini << EOF
            #!/usr/bin/env bash
            exec ${pkgs.nodejs_22}/bin/node $out/share/gemini-cli/bundle/gemini.js "\$@"
            EOF
            chmod +x $out/bin/gemini
            
            # Also create direct symlink
            ln -s $out/share/gemini-cli/bundle/gemini.js $out/bin/gemini.js
          '';
          
          meta = with pkgs.lib; {
            description = "Google Gemini CLI tool (built from source)";
            homepage = "https://github.com/google-gemini/gemini-cli";
            license = licenses.asl20;
            maintainers = [ ];
            platforms = platforms.all;
          };
        };
      in
      {
        # Default package tries to use existing bundle, fallback available
        packages = {
          default = gemini-cli;
          gemini-cli = gemini-cli;
          gemini-cli-with-build = gemini-cli-with-build;
          node2nix = node2nix-src.packages.${system}.default;
        };
        
        devShells.default = pkgs.mkShell {
          buildInputs = [
            pkgs.nodejs_22
            node2nix-src.packages.${system}.default
          ];
        };
        
        # Add apps for easy running
        apps = {
          default = {
            type = "app";
            program = "${gemini-cli}/bin/gemini";
          };
          gemini = {
            type = "app"; 
            program = "${gemini-cli}/bin/gemini";
          };
          gemini-with-build = {
            type = "app";
            program = "${gemini-cli-with-build}/bin/gemini";
          };
        };
      }
    );
}