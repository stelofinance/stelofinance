# Stelo Finance, the leading finance platform of [BitCraft](https://bitcraftonline.com/)
- Store in-game assets or derivation of such in digital accounts
- Transact with any other player, anytime, no matter where in-game they are (if they even are in-game)
- Build financial applications and tools on top!

## Development
1. Use the Nix Flake shell
2. Run `task live`. This will create a hot-reloading dev environment

### DB
Currently testing out using [Turso](https://github.com/tursodatabase/turso) for the db. It's a full Rust rewrite of SQLite with the goal of extending it.

Migrations are done with [Goose](https://github.com/pressly/goose). Migrations are in `database/migrations/*`.

Queries are handled with [SQLC](https://sqlc.dev/). Queries are located in `database/queries/*`

### NATS (& JetStream)
During development, JetStream data is stored in `tmp/js`.

### Environment Secrets
The required ENV secrets are stored in `.env` at the project root, and are as follows:

- `ENV`: "dev" or "prod"
- `PORT`: Port for the web server to run on, such as "8080"
- `JS_DIR`: Directory to store JetStream data
- `GOOSE_DRIVER`: "sqlite3"
- `GOOSE_DBSTRING`: DB connection URI string (ex `./tmp/dev.db`)
- `GOOSE_MIGRATION_DIR`: "./database/migrations"
- `TURSO_FILE`: Same as `GOOSE_DBSTRING`, the DB file location
