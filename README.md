# Stelo Finance, the leading finance platform of [BitCraft](https://bitcraftonline.com/)
- Store in-game assets in digital accounts
- Transact with any other player, anytime, no matter where in-game they are
- Manage logistics of assets using a first party warehousing system

## Development
1. Use Nix Flake shell
2. Log into fly account with `fly auth login`
3. Connect to your fly private network via wireguard
4. Run `task live` (note, ensure the postgres cluster is running)
  - Note, a Taskfile bug will kill the first startup, so save one of the files to cause it to restart. (issue [#2202](https://github.com/go-task/task/issues/2202))

### DB Migrations
Database migrations are done with [Goose](https://github.com/pressly/goose). Migrations are in `database/migrations/*`.

### DB Queries
Queries are handled with [SQLC](https://sqlc.dev/). Queries are located in `database/queries/*`

### NATS (& JetStream)
During development, JetStream data is stored in `tmp/js`.

### Environment Secrets
The required ENV secrets are stored in `.env` at the project root, and are as follows:

- `ENV`: "dev" or "prod"
- `PORT`: Port for the web server to run on, such as "8080"
- `GOOSE_DRIVER`: "postgres"
- `GOOSE_DBSTRING`: DB connection URI string
- `GOOSE_MIGRATION_DIR`: "./database/migrations"
- `GOTH_KEY`: Key for Goth
- `DISCORD_CLIENT_ID`: The Discord client ID for OAuth
- `DISCORD_CLIENT_SECRET`: The Discrod client secret for OAuth
- `POSTGRES_URI`: Same as `GOOSE_DBSTRING`, the DB connection URI string
