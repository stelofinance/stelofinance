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

      # Use the same overlay toolchain for packaging Rust tools (e.g. topcoat-cli).
      rustPlatform = pkgs.makeRustPlatform {
        cargo = rustToolchain;
        rustc = rustToolchain;
      };

      topcoatCliVersion = "0.5.0";
      topcoat-cli = rustPlatform.buildRustPackage {
        pname = "topcoat-cli";
        version = topcoatCliVersion;

        src = pkgs.fetchCrate {
          pname = "topcoat-cli";
          version = topcoatCliVersion;
          hash = "sha256-Z/Z9KCIj6M36MvKOpC3b0S24MPpov2nQCdNCg1Fp98U=";
        };

        # Vendor hash of crates.io deps from Cargo.lock; rebuild to refresh when bumping version.
        cargoHash = "sha256-9KeF31rlUp5EuirfvIN7Cs0KUuZFvirYyQWFB4Ud5CE=";

        # Skip tests: crate tests pull in the full topcoat framework and are not needed for the CLI bin.
        doCheck = false;

        meta = with pkgs.lib; {
          description = "Topcoat CLI (dev server, fmt, asset bundling)";
          homepage = "https://github.com/tokio-rs/topcoat";
          license = licenses.mit;
          mainProgram = "topcoat";
        };
      };

      # ---------------------------------------------------------------------------
      # SpacetimeDB: pinned GitHub release binaries (not nixpkgs / not built from source).
      # Update `spacetimeVersion` + hashes when bumping.
      # Release assets: https://github.com/clockworklabs/SpacetimeDB/releases
      # ---------------------------------------------------------------------------
      spacetimeVersion = "2.10.2";

      spacetimeAsset = {
        x86_64-linux = {
          triple = "x86_64-unknown-linux-gnu";
          hash = "sha256-Xb4/J6jJ4nodmqhukQHw629c9a6+oCQLWJdQ5CSmlb0=";
        };
        aarch64-linux = {
          triple = "aarch64-unknown-linux-gnu";
          hash = "sha256-FhI4JBuW1E6YE9AcMddxSSeQxCdSoqoj5NU4Kq9mF/U=";
        };
        x86_64-darwin = {
          triple = "x86_64-apple-darwin";
          hash = "sha256-bNKyvmKZKwqAHW/HcEjGe/yMzYoldO1K805Crzg6lJY=";
        };
        aarch64-darwin = {
          triple = "aarch64-apple-darwin";
          hash = "sha256-U/g8kg4UKRixz/1Z4zvaiYvsV0L8TpH4xU/hNPJ/P3Q=";
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

      # spacetime CLI writes $XDG_CONFIG_HOME/spacetime/cli.toml (else $HOME/.config).
      # Nix sandbox HOME is /homeless-shelter; Config::save unwraps EACCES.
      spacetimeCliHome = ''
        export HOME="$TMPDIR"
        export XDG_CONFIG_HOME="$TMPDIR/.config"
        export XDG_DATA_HOME="$TMPDIR/.local/share"
        export XDG_CACHE_HOME="$TMPDIR/.cache"
        mkdir -p "$XDG_CONFIG_HOME/spacetime" "$XDG_DATA_HOME/spacetime" "$XDG_CACHE_HOME"
      '';

      # Interactive + agent shells: Rust edge, SpacetimeDB module, Topcoat CLI.
      commonBuildInputs = with pkgs; [
        go-task
        curl

        # Rust (stable + wasm32) for the edge crate and SpacetimeDB modules.
        rustToolchain
        # Build-script crates need a host linker even when targeting wasm32.
        stdenv.cc
        # spacetimedb-lib build.rs runs `git rev-parse HEAD` outside Nix package builds.
        git
        pkg-config
        openssl

        spacetimedb

        # `spacetime build` release path invokes wasm-opt when available
        binaryen

        # Topcoat frontend framework CLI (`topcoat dev`, fmt, assets)
        topcoat-cli
        # Edge `build.rs` looks up `tailwindcss` on PATH (Topcoat Tailwind feature).
        tailwindcss_4
      ];

      # Remote files Topcoat would otherwise download during `cargo build` /
      # `topcoat asset bundle`. Prefetched so the edge derivation stays offline.
      fontNormal = pkgs.fetchurl {
        url = "https://cdn.jsdelivr.net/fontsource/fonts/source-code-pro:vf@5.3.0/latin-wght-normal.woff2";
        hash = "sha256-i3dKqlE3o470D0rJ0225pe7hUrL2ZYnf3IL/AH/IcTU=";
      };
      fontItalic = pkgs.fetchurl {
        url = "https://cdn.jsdelivr.net/fontsource/fonts/source-code-pro:vf@5.3.0/latin-wght-italic.woff2";
        hash = "sha256-l/ATMq7aZCovKYAwID3U3XKALAKIqycF0z2vaRJ45WM=";
      };
      datastarJs = pkgs.fetchurl {
        url = "https://cdn.jsdelivr.net/gh/starfederation/datastar@1.0.2/bundles/datastar.js";
        hash = "sha256-KDfYes9u4LqOTmN2WSbCWpjWOIOwL4i+GUqGuB0/0ko=";
      };
      lucideJson = pkgs.fetchurl {
        url = "https://cdn.jsdelivr.net/npm/@iconify-json/lucide@1.2.1/icons.json";
        hash = "sha256-Zex+nLl42OdNtlvtj22tIiIb01F6YwWqB6NZsvTtlk4=";
      };

      # Module WASM, used only to generate `src/module_bindings` (gitignored).
      moduleWasm = rustPlatform.buildRustPackage {
        pname = "stelofinance-module";
        version = "0.1.0";
        src = ./spacetimedb;
        cargoLock.lockFile = ./spacetimedb/Cargo.lock;
        doCheck = false;
        dontFixup = true;

        nativeBuildInputs = with pkgs; [ git binaryen spacetimedb ];

        # cargoBuildHook always adds the host triple; uuid then fails on wasm32.
        # `spacetime build` is the same path as local module iteration.
        buildPhase = ''
          runHook preBuild
          ${spacetimeCliHome}
          git init -q
          git config user.email "nix@localhost"
          git config user.name "nix"
          git add -A
          git commit -qm init --allow-empty
          spacetime build -p .
          runHook postBuild
        '';

        installPhase = ''
          runHook preInstall
          mkdir -p $out
          wasm=$(find . -path '*/wasm32-unknown-unknown/release/stelofinance.wasm' | head -n1)
          test -n "$wasm"
          cp "$wasm" $out/stelofinance.wasm
          runHook postInstall
        '';
      };

      edge = rustPlatform.buildRustPackage {
        pname = "stelofinance";
        version = "0.5.0";
        src = pkgs.lib.fileset.toSource {
          root = ./.;
          fileset = pkgs.lib.fileset.unions [
            ./Cargo.toml
            ./Cargo.lock
            ./build.rs
            ./src
            ./assets
            ./spacetimedb
            ./spacetime.json
            ./spacetime.dev.json
          ];
        };
        cargoLock.lockFile = ./Cargo.lock;
        doCheck = false;

        nativeBuildInputs = [
          pkgs.pkg-config
          pkgs.openssl
          pkgs.tailwindcss_4
          pkgs.git
          spacetimedb
          topcoat-cli
        ];
        buildInputs = [ pkgs.openssl ];

        preConfigure = ''
          ${spacetimeCliHome}
          mkdir -p src/module_bindings
          spacetime generate --lang rust --out-dir src/module_bindings \
            --bin-path ${moduleWasm}/stelofinance.wasm -y
        '';

        preBuild = ''
          host=$(rustc -vV | sed -n 's/^host: //p')
          root="''${CARGO_TARGET_DIR:-target}"
          for cache in "$root/topcoat/cache" "$root/$host/topcoat/cache"; do
            mkdir -p "$cache/iconify" "$cache/assets"
            cp -L ${lucideJson} "$cache/iconify/lucide.json"
            cp -L ${fontNormal} "$cache/assets/7c2b2cd0f7787a4f7fca29dfe8ca3768.woff2"
            cp -L ${fontItalic} "$cache/assets/060feb8861a393823696b65af728cdd2.woff2"
            cp -L ${datastarJs} "$cache/assets/ae876aed55234b0e7ad466cc9682385c.js"
          done
        '';

        postBuild = ''
          topcoat asset bundle --release -o bundled-assets
        '';

        postInstall = ''
          mkdir -p $out/bin/assets
          cp -R bundled-assets/. $out/bin/assets/
          test -f $out/bin/assets/manifest.toml
        '';

        meta = with pkgs.lib; {
          description = "Stelo Finance Topcoat edge";
          mainProgram = "stelofinance";
        };
      };

      container = pkgs.dockerTools.streamLayeredImage {
        name = "stelo";
        tag = "latest";
        contents = [
          edge
          pkgs.cacert
          pkgs.dockerTools.fakeNss
        ];
        config = {
          Cmd = [ "${edge}/bin/stelofinance" ];
          Env = [
            "HOST=0.0.0.0"
            "PORT=8080"
            "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
          ];
        };
      };
    in
    {
      packages = {
        inherit spacetimedb topcoat-cli edge container;
        module = moduleWasm;
        default = edge;
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
