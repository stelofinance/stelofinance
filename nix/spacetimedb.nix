# SpacetimeDB CLI + standalone server from the upstream GitHub release binaries
# (not nixpkgs, not built from source).
#
# To bump: change `version`, then refresh each hash with
#   nix store prefetch-file https://github.com/clockworklabs/SpacetimeDB/releases/download/v<version>/spacetime-<triple>.tar.gz
{
  lib,
  stdenv,
  fetchurl,
  autoPatchelfHook,
  zlib,
}:

let
  version = "2.15.0";

  assets = {
    x86_64-linux = {
      triple = "x86_64-unknown-linux-gnu";
      hash = "sha256-MO4dCtNF60ebwmlCPxwPLyH8VkzWNPyxFcJIRlNYsGI=";
    };
    aarch64-linux = {
      triple = "aarch64-unknown-linux-gnu";
      hash = "sha256-asZVJ2+a2CJ82unQbJDf+HSdb+EsHQ0nSb7q+2f9rtI=";
    };
    x86_64-darwin = {
      triple = "x86_64-apple-darwin";
      hash = "sha256-xjDyYmTi/gpPJ2Mkr7QOt25brEXChUYLJqHiRnX/05M=";
    };
    aarch64-darwin = {
      triple = "aarch64-apple-darwin";
      hash = "sha256-8Bj2V0IAb8POTaqeZ28nEFibK7yo6gg45nzZV3Mjcz4=";
    };
  };

  system = stdenv.hostPlatform.system;
  asset = assets.${system} or (throw "SpacetimeDB release binaries are not available for ${system}");
in
stdenv.mkDerivation {
  pname = "spacetimedb";
  inherit version;

  src = fetchurl {
    url = "https://github.com/clockworklabs/SpacetimeDB/releases/download/v${version}/spacetime-${asset.triple}.tar.gz";
    inherit (asset) hash;
  };

  # Flat tarball: spacetimedb-cli and spacetimedb-standalone at the root.
  sourceRoot = ".";

  nativeBuildInputs = lib.optionals stdenv.hostPlatform.isLinux [ autoPatchelfHook ];

  # The CLI links zlib; both binaries need libgcc / libstdc++.
  buildInputs = lib.optionals stdenv.hostPlatform.isLinux [
    stdenv.cc.cc.lib
    zlib
  ];

  installPhase = ''
    runHook preInstall
    install -Dm755 spacetimedb-cli $out/bin/spacetime
    install -Dm755 spacetimedb-standalone $out/bin/spacetimedb-standalone
    ln -s spacetime $out/bin/spacetimedb-cli
    runHook postInstall
  '';

  meta = {
    description = "SpacetimeDB CLI and standalone server (upstream release binaries)";
    homepage = "https://github.com/clockworklabs/SpacetimeDB";
    # BSL 1.1, which nixpkgs treats as unfree.
    license = lib.licenses.bsl11;
    sourceProvenance = [ lib.sourceTypes.binaryNativeCode ];
    platforms = builtins.attrNames assets;
    mainProgram = "spacetime";
  };
}
