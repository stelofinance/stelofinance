-- +goose Up
ALTER TABLE transaction ADD COLUMN status INT NOT NULL DEFAULT 0;

-- +goose Down
ALTER TABLE transaction DROP COLUMN status;
