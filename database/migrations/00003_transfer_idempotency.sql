-- +goose Up
CREATE TABLE IF NOT EXISTS transfer_idempotency
(
    account_id INTEGER NOT NULL REFERENCES account(id),
    key TEXT NOT NULL,
    transfer_id INTEGER NOT NULL REFERENCES transfer(id),
    request_hash TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT (datetime('now', 'subsec')),
    PRIMARY KEY (account_id, key)
);

-- +goose Down
DROP TABLE IF EXISTS transfer_idempotency;
