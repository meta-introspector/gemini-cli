{ pkgs }:
pkgs.mkShell {
  buildInputs = [ pkgs.nodejs_latest pkgs.which pkgs.git ];
}
