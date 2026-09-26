# Stelo Finance, the leading finance platform of [BitCraft](https://bitcraftonline.com/)
- Store in-game assets or derivation of such in digital accounts
- Transact with any other player, anytime, no matter where in-game they are (if they even are in-game)
- Build financial applications and tools on top!

## API Documentation - WIP
For API documentation, see [here](docs/api)

## Development

Domain logic lives in the SpacetimeDB module (`spacetimedb/`). The website is a stateless Rust [Topcoat](https://github.com/tokio-rs/topcoat) edge (`src/`) that talks to SpacetimeDB as the logged-in user.

1. Use the Nix flake shell (`direnv` or `nix develop`)
2. Copy `.env.example` to `.env` and fill BitAuth (and optional SpacetimeAuth) values
3. Run `task live`

That starts local SpacetimeDB (data under `tmp/spacetimedb`), watches the module (rebuild / publish / regenerate `src/module_bindings`), and runs `topcoat dev` on **port 8080**. Topcoat’s default port is 3000, which collides with SpacetimeDB.

| Command | What |
|---------|------|
| `task live` | Full stack |
| `task stdb` | SpacetimeDB only |
| `task stdb:dev` | Module watch only (needs STDB) |
| `task edge` | Topcoat only (needs STDB + published module) |

Wipe `tmp/spacetimedb` if snapshot or identity errors appear after a schema break.

### Environment

Loaded from `.env` at the repo root (see `.env.example`).
