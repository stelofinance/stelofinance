{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, flake-utils, rust-overlay, ... }:
  flake-utils.lib.eachDefaultSystem (system:
    let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [ (import rust-overlay) ];
        # SpacetimeDB is BSL 1.1 (nixpkgs treats BSL as unfree).
        config.allowUnfree = true;
      };

      # Stable Rust + wasm32-unknown-unknown (required by `spacetime build` for modules).
      # Uses rust-overlay's prebuilt toolchains rather than compiling rustc from source.
      rustToolchain = pkgs.rust-bin.stable.latest.default.override {
        extensions = [
          "rust-src"
          "rust-analyzer"
          "clippy"
          "rustfmt"
        ];
        targets = [ "wasm32-unknown-unknown" ];
      };

      # ---------------------------------------------------------------------------
      # SpacetimeDB: pinned GitHub release binaries (not nixpkgs / not built from source).
      # Update `spacetimeVersion` + hashes when bumping.
      # Release assets: https://github.com/clockworklabs/SpacetimeDB/releases
      # ---------------------------------------------------------------------------
      spacetimeVersion = "2.7.0-hotfix3";

      spacetimeAsset = {
        x86_64-linux = {
          triple = "x86_64-unknown-linux-gnu";
          hash = "sha256-0lxsnk7F1S/kPNfTeq9DufTU+i0ii27NJBunW9rpmDE=";
        };
        aarch64-linux = {
          triple = "aarch64-unknown-linux-gnu";
          hash = "sha256-vv0T95bZPQxbfBdRxz0Vz9CCGZDpXdkFm/FWPTRLqeo=";
        };
        x86_64-darwin = {
          triple = "x86_64-apple-darwin";
          hash = "sha256-xSt1ntEEjFKhymYDCsKcoHjfANyxP1Fu+Y4VsrRZMi8=";
        };
        aarch64-darwin = {
          triple = "aarch64-apple-darwin";
          hash = "sha256-Wovx76DRDBE4BYQ+R3B56iTNHNBFBJ45JSUxwAl79Rc=";
        };
      }.${system} or (throw "SpacetimeDB release binaries are not packaged for ${system}");

      spacetimedb = pkgs.stdenv.mkDerivation {
        pname = "spacetimedb";
        version = spacetimeVersion;

        src = pkgs.fetchurl {
          url = "https://github.com/clockworklabs/SpacetimeDB/releases/download/v${spacetimeVersion}/spacetime-${spacetimeAsset.triple}.tar.gz";
          hash = spacetimeAsset.hash;
        };

        # Flat tarball (spacetimedb-cli + spacetimedb-standalone at root)
        dontUnpack = true;

        nativeBuildInputs = pkgs.lib.optionals pkgs.stdenv.isLinux [
          pkgs.autoPatchelfHook
        ];

        # CLI needs zlib; both need libgcc / libstdc++ on Linux.
        buildInputs = pkgs.lib.optionals pkgs.stdenv.isLinux [
          pkgs.stdenv.cc.cc.lib
          pkgs.zlib
        ];

        installPhase = ''
          runHook preInstall
          mkdir -p $out/bin
          tar -xzf $src -C $TMPDIR
          install -m755 $TMPDIR/spacetimedb-cli $out/bin/spacetime
          install -m755 $TMPDIR/spacetimedb-standalone $out/bin/spacetimedb-standalone
          ln -s spacetime $out/bin/spacetimedb-cli
          runHook postInstall
        '';

        meta = with pkgs.lib; {
          description = "SpacetimeDB CLI and standalone server (upstream release binaries)";
          homepage = "https://github.com/clockworklabs/SpacetimeDB";
          license = licenses.bsl11;
          platforms = [
            "x86_64-linux"
            "aarch64-linux"
            "x86_64-darwin"
            "aarch64-darwin"
          ];
          mainProgram = "spacetime";
        };
      };

      # Packages shared by interactive + agent shells (Go edge + Rust/STDB module work).
      commonBuildInputs = with pkgs; [
        # Existing Go app toolchain
        tailwindcss_4
        go-task
        sqlc
        go
        goose
        watchexec
        litecli

        # Rust (stable + wasm32) for SpacetimeDB modules
        rustToolchain
        pkg-config
        openssl

        spacetimedb

        # `spacetime build` release path invokes wasm-opt when available
        binaryen
      ];

      app = pkgs.buildGoModule {
        pname = "app";
        version = "0.4.0";
        src = ./.;
        subPackages = [ "cmd/app" ];

        nativeBuildInputs = with pkgs; [ sqlc tailwindcss_4 ];

        env.CGO_ENABLED = 0;
        vendorHash = "sha256-Y2XvAQsHz9FQv/x3zNwEOvEowpsUxnH4XnbVmPR+RIA=";

        postPatch = ''
          tailwindcss -i web/styles/tw-input.css -o web/static/tw-output.css --minify
          sqlc generate
        '';
      };

      container = pkgs.dockerTools.streamLayeredImage {
        name = "stelo";
        tag = "latest";
        contents = [ app pkgs.cacert ];
        config = {
          Cmd = [ "${app}/bin/app" ];
        };
      };
    in
    {
      packages = {
        inherit app container spacetimedb;
        default = app;
      };

      devShells.default = pkgs.mkShell {
        buildInputs = commonBuildInputs;
      };

      # Agent shell: same toolchain as interactive so refactor work is fully available.
      devShells.agent = pkgs.mkShell {
        buildInputs = commonBuildInputs;
      };
    });
}
