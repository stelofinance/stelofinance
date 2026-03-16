-- name: InsertUser :one
INSERT INTO "user" (bitcraft_username, created_at) VALUES (?, ?) RETURNING id;

-- name: GetUserByUsername :one
SELECT * FROM "user" WHERE bitcraft_username = ?;

-- name: GetUserById :one
SELECT * FROM "user" WHERE id = ?;
