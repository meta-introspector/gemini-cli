{
  description = "Development shell for gemini-cli source code";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
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

            src = self;

            

            npmDepsHash = "sha256-q7E5YEMjHs9RvfT4ctzltqHr/+cCh3M+G6D2MkLiJFg=";

            buildInputs = [ pkgs.ripgrep ];

            preConfigure = ''
              ${pkgs.bash}/bin/bash ./scripts/generate-git-info.sh "${finalAttrs.src.rev or "dirty"}"
            '';

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
