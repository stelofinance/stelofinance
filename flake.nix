{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs, rust-overlay, ... }:
    let
      inherit (nixpkgs) lib;

      # Every platform SpacetimeDB publishes Unix release binaries for.
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      forAllSystems =
        f:
        lib.genAttrs systems (
          system:
          f (
            import nixpkgs {
              inherit system;
              overlays = [ (import rust-overlay) ];
              # SpacetimeDB is BSL 1.1, which nixpkgs treats as unfree.
              config.allowUnfreePredicate = pkg: lib.getName pkg == "spacetimedb";
            }
          )
        );

      perSystem =
        pkgs:
        let
          # Stable Rust with wasm32-unknown-unknown (`spacetime build` needs it for
          # modules). rust-overlay ships prebuilt toolchains, so rustc is not
          # compiled from source.
          #
          # Pinned below 1.100: Cargo 1.100 lays build scripts out as
          # `build/<pkg>/<hash>/out`, which Topcoat 0.12.1's cache lookup does not
          # recognize, so it downloads assets at build time and the offline edge
          # build fails.
          rustToolchain = pkgs.rust-bin.stable."1.99.1".default.override {
            extensions = [
              "rust-src"
              "rust-analyzer"
              "clippy"
              "rustfmt"
            ];
            targets = [ "wasm32-unknown-unknown" ];
          };

          # Package Rust code with the same toolchain the dev shell uses.
          rustPlatform = pkgs.makeRustPlatform {
            cargo = rustToolchain;
            rustc = rustToolchain;
          };

          # The triple buildRustPackage passes to `cargo build --target`.
          rustTarget = pkgs.stdenv.hostPlatform.rust.rustcTarget;

          spacetimedb = pkgs.callPackage ./nix/spacetimedb.nix { };

          # `spacetime build` runs `wasm-opt -all` when it is on PATH. Binaryen
          # 133's `-all` includes the compact import section, which the
          # SpacetimeDB 2.15 CLI cannot parse ("invalid leading byte (0x7e) for
          # external kind"), so publish and generate fail. Appending the disable
          # flag overrides `-all`. Drop this once the CLI parses compact imports.
          wasm-opt = pkgs.writeShellScriptBin "wasm-opt" ''
            exec ${pkgs.binaryen}/bin/wasm-opt "$@" --disable-compact-imports
          '';
          topcoat-cli = pkgs.callPackage ./nix/topcoat-cli.nix { inherit rustPlatform; };

          # The spacetime CLI saves $XDG_CONFIG_HOME/spacetime/cli.toml and panics
          # if it cannot. The sandbox HOME (/homeless-shelter) is not writable.
          spacetimeSandboxHome = ''
            export HOME="$TMPDIR"
            export XDG_CONFIG_HOME="$TMPDIR/.config"
            export XDG_DATA_HOME="$TMPDIR/.local/share"
            export XDG_CACHE_HOME="$TMPDIR/.cache"
            mkdir -p "$XDG_CONFIG_HOME/spacetime" "$XDG_DATA_HOME/spacetime" "$XDG_CACHE_HOME"
          '';

          # Remote files Topcoat would otherwise download during `cargo build` /
          # `topcoat asset bundle`, laid out like Topcoat's cache directory so the
          # edge build stays offline.
          topcoatCache = pkgs.linkFarm "topcoat-cache" {
            "iconify/lucide.json" = pkgs.fetchurl {
              url = "https://cdn.jsdelivr.net/npm/@iconify-json/lucide@1.2.1/icons.json";
              hash = "sha256-Zex+nLl42OdNtlvtj22tIiIb01F6YwWqB6NZsvTtlk4=";
            };
            "assets/7c2b2cd0f7787a4f7fca29dfe8ca3768.woff2" = pkgs.fetchurl {
              url = "https://cdn.jsdelivr.net/fontsource/fonts/source-code-pro:vf@5.3.0/latin-wght-normal.woff2";
              hash = "sha256-i3dKqlE3o470D0rJ0225pe7hUrL2ZYnf3IL/AH/IcTU=";
            };
            "assets/060feb8861a393823696b65af728cdd2.woff2" = pkgs.fetchurl {
              url = "https://cdn.jsdelivr.net/fontsource/fonts/source-code-pro:vf@5.3.0/latin-wght-italic.woff2";
              hash = "sha256-l/ATMq7aZCovKYAwID3U3XKALAKIqycF0z2vaRJ45WM=";
            };
          };

          # `topcoat asset bundle` runs its own `cargo build`, strips CARGO*/RUSTFLAGS
          # and does not pass `--target`. buildRustPackage always builds with
          # `--target`, so without this the inner build gets a different OUT_DIR.
          # `stylesheet!()` hashes that path into the asset id, so the manifest would
          # miss the ids baked into the installed binary and the first HTML response
          # would panic (`failed to resolve asset`). This shim makes the inner build
          # reuse the outer one.
          bundleCargo = pkgs.writeShellScriptBin "cargo" ''
            if [ "$1" = build ]; then
              exec ${rustToolchain}/bin/cargo "$@" --offline --target ${rustTarget}
            fi
            exec ${rustToolchain}/bin/cargo "$@"
          '';

          # Fails the edge build if the binary references an asset id that is not in
          # the bundled manifest. See nix/check-asset-ids.rs.
          checkAssetIds = pkgs.runCommandCC "check-asset-ids" { nativeBuildInputs = [ rustToolchain ]; } ''
            rustc --edition 2024 -O -o $out ${./nix/check-asset-ids.rs}
          '';

          # Module WASM. Used to generate `src/module_bindings` (gitignored) for the
          # edge, and published to Maincloud by CI.
          module = rustPlatform.buildRustPackage {
            pname = "stelofinance-module";
            version = (lib.importTOML ./spacetimedb/Cargo.toml).package.version;
            src = ./spacetimedb;
            cargoLock.lockFile = ./spacetimedb/Cargo.lock;
            doCheck = false;
            dontFixup = true;

            nativeBuildInputs = [
              pkgs.git
              wasm-opt
              spacetimedb
            ];

            # cargoBuildHook always passes the host triple, and uuid fails to build
            # for wasm32 that way. `spacetime build` is what local iteration uses.
            # spacetimedb-lib's build.rs needs a git repo with a HEAD commit.
            buildPhase = ''
              runHook preBuild
              ${spacetimeSandboxHome}
              git init -q
              git -c user.email=nix@localhost -c user.name=nix commit -q --allow-empty -m init
              spacetime build -p .
              runHook postBuild
            '';

            installPhase = ''
              runHook preInstall
              wasm=$(find . -path '*/wasm32-unknown-unknown/release/stelofinance.wasm' -print -quit)
              test -n "$wasm"
              install -Dm644 "$wasm" $out/stelofinance.wasm
              runHook postInstall
            '';
          };

          edge = rustPlatform.buildRustPackage {
            pname = "stelofinance";
            version = (lib.importTOML ./Cargo.toml).package.version;
            src = lib.fileset.toSource {
              root = ./.;
              fileset = lib.fileset.unions [
                ./Cargo.toml
                ./Cargo.lock
                ./build.rs
                ./src
                ./assets
                ./spacetimedb
                ./spacetime.json
              ];
            };
            cargoLock.lockFile = ./Cargo.lock;
            # CI builds this (and so runs the unit tests) before publishing the module.
            doCheck = true;

            nativeBuildInputs = [
              pkgs.pkg-config
              pkgs.tailwindcss_4
              pkgs.git
              spacetimedb
              topcoat-cli
            ];
            buildInputs = [ pkgs.openssl ];

            preConfigure = ''
              ${spacetimeSandboxHome}
              spacetime generate --lang rust --out-dir src/module_bindings \
                --bin-path ${module}/stelofinance.wasm -y
            '';

            preBuild = ''
              for cache in target/topcoat/cache target/${rustTarget}/topcoat/cache; do
                mkdir -p "$cache"
                cp -rL --no-preserve=mode ${topcoatCache}/. "$cache/"
              done
            '';

            postBuild = ''
              PATH="${bundleCargo}/bin:$PATH" topcoat asset bundle --release -o bundled-assets
            '';

            postInstall = ''
              cp -R bundled-assets $out/bin/assets
            '';

            doInstallCheck = true;
            installCheckPhase = ''
              runHook preInstallCheck
              ${checkAssetIds} $out/bin/stelofinance $out/bin/assets/manifest.toml
              runHook postInstallCheck
            '';

            meta = {
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
              Cmd = [ (lib.getExe edge) ];
              Env = [
                "HOST=0.0.0.0"
                "PORT=8080"
                "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
              ];
            };
          };

          devShell = pkgs.mkShell {
            packages = [
              rustToolchain
              pkgs.pkg-config
              # spacetimedb-lib's build.rs runs `git rev-parse HEAD`.
              pkgs.git
              pkgs.go-task
              pkgs.curl

              spacetimedb
              # `spacetime build --release` runs wasm-opt when it is on PATH.
              wasm-opt

              topcoat-cli
              # The edge build.rs looks up `tailwindcss` on PATH (Topcoat Tailwind feature).
              pkgs.tailwindcss_4
            ];
            buildInputs = [ pkgs.openssl ];
          };
        in
        {
          packages = {
            inherit
              spacetimedb
              topcoat-cli
              module
              edge
              container
              ;
            default = edge;
          };

          devShells = {
            default = devShell;
            # The sandbox shell coding agents in this repo run in. Keep it identical
            # to the interactive shell so agents have the full toolchain.
            agent = devShell;
          };

          formatter = pkgs.nixfmt;
        };

      outputs = forAllSystems perSystem;
    in
    {
      packages = lib.mapAttrs (_: o: o.packages) outputs;
      devShells = lib.mapAttrs (_: o: o.devShells) outputs;
      formatter = lib.mapAttrs (_: o: o.formatter) outputs;
    };
}
