-- +goose Up
CREATE TABLE IF NOT EXISTS "user"
(
    id INTEGER PRIMARY KEY,
    bitcraft_username TEXT NOT NULL UNIQUE,
    bitcraft_id TEXT NOT NULL UNIQUE,
    created_at TIMESTAMP NOT NULL
);
-- +goose Down
DROP TABLE IF EXISTS "user";
