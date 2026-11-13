# Topcoat CLI (`topcoat dev`, `topcoat fmt`, asset bundling).
#
# Must match the `topcoat` crate version in Cargo.toml. When bumping, update
# `version`, then `hash` and `cargoHash` (set them to `lib.fakeHash` and
# rebuild to get the new values).
{
  lib,
  rustPlatform,
  fetchCrate,
}:

rustPlatform.buildRustPackage (finalAttrs: {
  pname = "topcoat-cli";
  version = "0.12.1";

  src = fetchCrate {
    inherit (finalAttrs) pname version;
    hash = "sha256-v1t4H/i0D64yoRPFHks/tXYW81FFx/7j4DZfKGxLrvU=";
  };

  cargoHash = "sha256-xdnW5WjmbUuvmsZAEKHDemE9nh5qLGHcrEPGoKAyFAA=";

  # The crate's tests pull in the whole Topcoat framework; only the binary is needed.
  doCheck = false;

  meta = {
    description = "Topcoat CLI (dev server, fmt, asset bundling)";
    homepage = "https://github.com/tokio-rs/topcoat";
    license = lib.licenses.mit;
    mainProgram = "topcoat";
  };
})
