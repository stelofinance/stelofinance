-- name: InsertUser :one
INSERT INTO "user" (bitcraft_username, bitcraft_id, created_at) VALUES (?, ?, ?) RETURNING id;

-- name: GetUserByUsername :one
SELECT * FROM "user" WHERE bitcraft_username = ?;

-- name: GetUserByBitCraftId :one
SELECT * FROM "user" WHERE bitcraft_id = ?;

-- name: GetUserById :one
SELECT * FROM "user" WHERE id = ?;
