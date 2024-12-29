{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };
  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem
    (system:
      let
        system = "x86_64-linux";
        pkgs = import nixpkgs {
          inherit system;
        };
      in
      with pkgs;
      {
        devShells.default = mkShell {
          buildInputs = [
            tailwindcss
            go-task
            sqlc
            go
          ] ++ (if builtins.getEnv "NIX_BUILD_SHELL" != "1" then [
            goose
          ] else []);
        };
      }
    );
}
