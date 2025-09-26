{
  description = "Development shell for gemini-cli source code";

  inputs = {
#    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
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
                {
                                                      default = pkgs.buildNpmPackage (finalAttrs: {
                                                        pname = "gemini-cli";
                                                        version = "0.3.4";
                                                      });                              test-node2nix-failure = import ./test_node2nix_failure.nix { inherit pkgs; };            src = self;

            

            npmDepsHash = "sha256-HxBWuaYo25WGxEcXNSOC9yz3JIpfmDb7aryQwp0WtMk=";

            preConfigure = ''
              export PKG_CONFIG_PATH=${pkgs.libsecret.dev}/lib/pkgconfig:$PKG_CONFIG_PATH
              export NIX_CFLAGS_COMPILE="-I${pkgs.libsecret.dev}/include/libsecret-1 -I${pkgs.glib.dev}/include/gio-unix-2.0 -I${pkgs.glib.dev}/include -I${pkgs.glib.dev}/include/glib-2.0 -I${pkgs.glib}/lib/glib-2.0/include"
              export NIX_LDFLAGS="-L${pkgs.libsecret}/lib -L${pkgs.glib}/lib -lsecret-1 -lgio-2.0 -lgobject-2.0 -lglib-2.0"
              export npm_config_keytar_build_from_source=false
              ${pkgs.bash}/bin/bash ./scripts/generate-git-info.sh "${finalAttrs.src.rev or "dirty"}"
            '';

            nativeBuildInputs = [ pkgs.ripgrep pkgs.pkg-config pkgs.libsecret pkgs.glib pkgs.gcc pkgs.gnumake ];



            installPhase = ''
              runHook preInstall
              mkdir -p $out/{bin,share/gemini-cli}

              cp -r node_modules $out/share/gemini-cli/

              rm -f $out/share/gemini-cli/node_modules/@google/gemini-cli
              rm -f $out/share/gemini-cli/node_modules/@google/gemini-cli-core
              rm -f $out/share/gemini-cli/node_modules/@google/gemini-cli-a2a-server
              rm -f $out/share/gemini-cli/node_modules/@google/gemini-cli-test-utils
              rm -f $out/share/gemini-cli/node_modules/gemini-cli-vscode-ide-companion
              cp -r packages/cli $out/share/gemini-cli/node_modules/@google/gemini-cli
              cp -r packages/core $out/share/gemini-cli/node_modules/@google/gemini-cli-core
              cp -r packages/a2a-server $out/share/gemini-cli/node_modules/@google/gemini-cli-a2a-server

              ln -s $out/share/gemini-cli/node_modules/@google/gemini-cli/dist/index.js $out/bin/gemini
              chmod +x "$out/bin/gemini"

              runHook postInstall
            '';

            meta = {
              description = "AI agent that brings the power of Gemini directly into your terminal";
              homepage = "https://github.com/google-gemini/gemini-cli";
              license = pkgs.lib.licenses.asl20;
              sourceProvenance = with pkgs.lib.sourceTypes; [ fromSource ];
              platforms = pkgs.lib.platforms.all;
              mainProgram = "gemini";
            };
          
        });

      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          default = pkgs.mkShell {
            buildInputs = [ pkgs.nodejs_latest pkgs.which pkgs.git ];
          };
        });
    };
}
