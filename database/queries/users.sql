-- name: InsertUser :one
INSERT INTO "user" (discord_id, discord_username, discord_pfp, created_at) VALUES ($1, $2, $3, $4) RETURNING id;

-- name: GetUser :one
SELECT * FROM "user" WHERE discord_id = $1;
