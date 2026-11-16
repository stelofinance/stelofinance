# Stelo Finance, the leading finance platform of [BitCraft](https://bitcraftonline.com/)
- Store in-game assets or derivation of such in digital accounts
- Transact with any other player, anytime, no matter where in-game they are (if they even are in-game)
- Build financial applications and tools on top!

## Building apps
Third-party apps connect directly to the SpacetimeDB module with a SpacetimeDB client SDK. Create an app identity on your profile page (`/app/me`), then grant it a role on an account from that account's Permissions section.

## Development

Domain logic lives in the SpacetimeDB module (`spacetimedb/`). The website is a stateless Rust [Topcoat](https://github.com/tokio-rs/topcoat) edge (`src/`) that talks to SpacetimeDB as the logged-in user.

1. Use the Nix flake shell (`direnv` or `nix develop`)
2. Copy `.env.example` to `.env` (it works as-is with the local `dev-oidc` login provider)
3. Run `task stdb` (local SpacetimeDB, data under `tmp/spacetimedb`)
4. In another terminal, run `spacetime dev -y`

`spacetime dev` watches the module (rebuild / publish / regenerate `src/module_bindings`) and runs the edge with `topcoat dev` on **port 8080**. Debug builds of the edge also load `.env` and run the local `dev-oidc` login provider on port 9090 (`src/dev_oidc.rs`), so there is nothing else to start. Release builds contain neither.

| Command | What |
|---------|------|
| `task stdb` | SpacetimeDB only |
| `task stdb:dev` | Module watch only (needs STDB) |
| `task edge` | Topcoat only (needs STDB + published module) |

Wipe `tmp/spacetimedb` if snapshot or identity errors appear after a schema break.

### Environment

Loaded from `.env` at the repo root by the edge itself, in debug builds only (see `.env.example`). Variables already set in the environment win.
