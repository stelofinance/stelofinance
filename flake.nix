{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
  flake-utils.lib.eachDefaultSystem (system:
    let
      pkgs = import nixpkgs { inherit system; };

      app = pkgs.buildGoModule {
        pname = "app";
        version = "0.3.0";
        src = ./.;
        subPackages = [ "cmd/app" ];

        nativeBuildInputs = with pkgs; [ sqlc tailwindcss_4 ];

        env.CGO_ENABLED = 0;
        vendorHash = "sha256-TVV5/k0x2dEkMNSegDc/uxllmzGvyHQ3OY2vyBtaBNQ=";

        postPatch = ''
          tailwindcss -i web/styles/tw-input.css -o web/static/tw-output.css --minify
          sqlc generate
        '';
      };
      migrate = pkgs.buildGoModule {
        pname = "migrate";
        version = "0.1.0";
        src = ./.;
        subPackages = [ "cmd/migrate" ];

        nativeBuildInputs = [ pkgs.sqlc ];

        env.CGO_ENABLED = 0;
        vendorHash = "sha256-TVV5/k0x2dEkMNSegDc/uxllmzGvyHQ3OY2vyBtaBNQ=";

        postPatch = ''
          sqlc generate
        '';
      };
      container = pkgs.dockerTools.streamLayeredImage {
        name = "stelo";
        tag = "latest";
        contents = [ migrate app ];
        config = {
          Cmd = [ "${app}/bin/app" ];
        };
      };
    in
    {
      packages = {
        inherit app container migrate;
        default = app;
      };

      devShells.default = with pkgs; mkShell {
        buildInputs = [
          tailwindcss_4
          go-task
          sqlc
          go
        ] ++ (if builtins.getEnv "NIX_BUILD_SHELL" != "1" then [
          # watchman # tailwind watch uses this
          goose
        ] else []);
      };
    });
}
