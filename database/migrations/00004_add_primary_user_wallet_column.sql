-- +goose Up
ALTER TABLE "user" ADD COLUMN wallet_id BIGINT REFERENCES wallet(id);

-- +goose Down
ALTER TABLE "user" DROP COLUMN wallet_id;
