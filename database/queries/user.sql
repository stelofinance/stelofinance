-- name: InsertUser :one
INSERT INTO "user" (discord_id, discord_username, discord_pfp, created_at) VALUES ($1, $2, $3, $4) RETURNING id;

-- name: GetUser :one
SELECT * FROM "user" WHERE discord_id = $1;

-- name: GetUserById :one
SELECT * FROM "user" WHERE id = $1;

-- name: GetUserByIdForUpdate :one
SELECT * FROM "user" WHERE id = $1 FOR UPDATE;

-- name: GetUserIdByDiscordName :one
SELECT id FROM "user" WHERE discord_username = $1;

-- name: UpdateUserWalletId :exec
UPDATE "user" SET wallet_id = $1 WHERE id = sqlc.arg(user_id);
