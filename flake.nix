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

      # Inlined: flakes do not see untracked paths, and this must run in the
      # edge build to prove the installed binary's asset ids are in the manifest.
      checkAssetIds = pkgs.writeText "check_asset_ids.rs" ''
        //! Fail the Nix edge build when the shipped binary references an asset id
        //! that is not in the bundle manifest installed next to it.
        //!
        //! Topcoat panics at request time (`failed to resolve asset`) if those drift.
        //! `stylesheet!()` bakes the absolute `OUT_DIR` into the id, so a manifest
        //! produced from any other cargo invocation will not match.

        use std::collections::BTreeSet;
        use std::env;
        use std::fs;
        use std::process::ExitCode;

        fn main() -> ExitCode {
            let mut args = env::args().skip(1);
            let Some(binary_path) = args.next() else {
                eprintln!("usage: check_asset_ids <binary> <manifest.toml>");
                return ExitCode::from(2);
            };
            let Some(manifest_path) = args.next() else {
                eprintln!("usage: check_asset_ids <binary> <manifest.toml>");
                return ExitCode::from(2);
            };

            let binary = match fs::read(&binary_path) {
                Ok(bytes) => bytes,
                Err(err) => {
                    eprintln!("read {binary_path}: {err}");
                    return ExitCode::from(2);
                }
            };
            let manifest = match fs::read_to_string(&manifest_path) {
                Ok(text) => text,
                Err(err) => {
                    eprintln!("read {manifest_path}: {err}");
                    return ExitCode::from(2);
                }
            };

            let embedded = embedded_asset_ids(&binary);
            let bundled = manifest_ids(&manifest);

            if embedded.is_empty() {
                eprintln!("no TOPCOAT_ASSET declarations found in {binary_path}");
                return ExitCode::from(1);
            }

            let missing: Vec<u64> = embedded.difference(&bundled).copied().collect();
            if missing.is_empty() {
                println!(
                    "asset ids match: {} embedded, {} in {}",
                    embedded.len(),
                    bundled.len(),
                    manifest_path
                );
                return ExitCode::SUCCESS;
            }

            eprintln!(
                "asset bundle does not match {binary_path}: {} id(s) embedded in the binary are missing from {manifest_path}",
                missing.len()
            );
            for id in missing {
                eprintln!("  missing id {id}");
            }
            eprintln!(
                "stylesheet!() includes OUT_DIR in the asset id. Bundle the same binary that is installed (same --target and profile)."
            );
            ExitCode::from(1)
        }

        fn manifest_ids(text: &str) -> BTreeSet<u64> {
            let mut ids = BTreeSet::new();
            for line in text.lines() {
                let Some(rest) = line.trim().strip_prefix("id = ") else {
                    continue;
                };
                if let Ok(id) = rest.trim().parse::<u64>() {
                    ids.insert(id);
                }
            }
            ids
        }

        fn embedded_asset_ids(binary: &[u8]) -> BTreeSet<u64> {
            const PREFIX: &[u8] = b"TOPCOAT_ASSET";
            let mut ids = BTreeSet::new();
            if binary.len() < PREFIX.len() {
                return ids;
            }
            let mut i = 0;
            while i + PREFIX.len() <= binary.len() {
                if &binary[i..i + PREFIX.len()] == PREFIX {
                    if let Some(id) = decode_asset_id(&binary[i..]) {
                        ids.insert(id);
                    }
                }
                i += 1;
            }
            ids
        }

        /// Decode one embedded `RawAsset` blob. Returns `None` when `buf` is not a
        /// real declaration (the prefix can also show up as the const-evaluated
        /// marker inside the library).
        fn decode_asset_id(buf: &[u8]) -> Option<u64> {
            let mut pos = b"TOPCOAT_ASSET".len();
            let id = read_u64(buf, &mut pos)?;
            let path = read_str(buf, &mut pos)?;
            let crate_name = read_str(buf, &mut pos)?;
            let manifest_dir = read_str(buf, &mut pos)?;
            let source_file = read_str(buf, &mut pos)?;
            for _ in 0..4 {
                read_str_opt(buf, &mut pos)?;
            }
            if path.is_empty()
                || path.len() > 4096
                || crate_name.is_empty()
                || crate_name.len() > 128
                || manifest_dir.len() > 4096
                || !source_file.ends_with(".rs")
                || !crate_name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                return None;
            }
            Some(id)
        }

        fn read_u64(buf: &[u8], pos: &mut usize) -> Option<u64> {
            let bytes = read_bytes(buf, pos, 8)?;
            Some(u64::from_le_bytes(bytes.try_into().ok()?))
        }

        fn read_str<'a>(buf: &'a [u8], pos: &mut usize) -> Option<&'a str> {
            let len = read_u16(buf, pos)? as usize;
            let bytes = read_bytes(buf, pos, len)?;
            std::str::from_utf8(bytes).ok()
        }

        fn read_str_opt<'a>(buf: &'a [u8], pos: &mut usize) -> Option<Option<&'a str>> {
            match read_bytes(buf, pos, 1)?[0] {
                0 => Some(None),
                1 => read_str(buf, pos).map(Some),
                _ => None,
            }
        }

        fn read_u16(buf: &[u8], pos: &mut usize) -> Option<u16> {
            let bytes = read_bytes(buf, pos, 2)?;
            Some(u16::from_le_bytes([bytes[0], bytes[1]]))
        }

        fn read_bytes<'a>(buf: &'a [u8], pos: &mut usize, n: usize) -> Option<&'a [u8]> {
            let end = pos.checked_add(n)?;
            if end > buf.len() {
                return None;
            }
            let slice = &buf[*pos..end];
            *pos = end;
            Some(slice)
        }
      '';

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

        # `topcoat asset bundle` runs its own `cargo build` and strips CARGO*/RUSTFLAGS,
        # and it does not pass `--target`. Nix compiles with `--target <triple>`, so
        # that second build gets a different OUT_DIR. `stylesheet!()` hashes that
        # absolute path into the asset id. The manifest would then not contain the
        # id baked into the binary this hook installs, and the first HTML response
        # panics (`failed to resolve asset`). Force the inner cargo to rebuild the
        # same target directory the install hook copies.
        postBuild = ''
          bin=$(find target -type f -path '*/release/stelofinance' ! -path '*/deps/*' -print -quit)
          test -n "$bin"
          test -x "$bin"
          release_dir=$(dirname "$bin")
          grand=$(dirname "$release_dir")
          target_args=
          if [ "$(basename "$grand")" != target ]; then
            target_args="--target $(basename "$grand")"
          fi

          wrap="$TMPDIR/cargo-wrap"
          mkdir -p "$wrap"
          real_cargo=$(command -v cargo)
          {
            printf '%s\n' '#!/bin/sh'
            printf '%s\n' 'if [ "$1" = "build" ]; then'
            printf '  exec %s "$@" --offline %s\n' "$real_cargo" "$target_args"
            printf '%s\n' 'fi'
            printf 'exec %s "$@"\n' "$real_cargo"
          } > "$wrap/cargo"
          chmod +x "$wrap/cargo"
          PATH="$wrap:$PATH" topcoat asset bundle --release -o bundled-assets
        '';

        postInstall = ''
          mkdir -p $out/bin/assets
          cp -R bundled-assets/. $out/bin/assets/
          test -f $out/bin/assets/manifest.toml
          test -x $out/bin/stelofinance
          rustc --edition 2021 -O -o "$TMPDIR/check_asset_ids" ${checkAssetIds}
          "$TMPDIR/check_asset_ids" "$out/bin/stelofinance" "$out/bin/assets/manifest.toml"
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
