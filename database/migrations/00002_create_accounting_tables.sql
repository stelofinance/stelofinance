-- +goose Up
CREATE TABLE IF NOT EXISTS ledger
(
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    asset_scale INTEGER NOT NULL,
    code INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS account
(
    id INTEGER PRIMARY KEY,
    address TEXT NOT NULL,
    webhook TEXT,
    user_id INTEGER REFERENCES "user"(id),

    debits_pending INTEGER NOT NULL,
    debits_posted INTEGER NOT NULL,
    credits_pending INTEGER NOT NULL,
    credits_posted INTEGER NOT NULL,

    ledger_id INTEGER NOT NULL REFERENCES ledger(id),
    code INTEGER NOT NULL,
    flags INTEGER NOT NULL,
    created_at DATETIME NOT NULL DEFAULT (datetime('now', 'subsec')),

    -- Users can only have one primary account for each ledger type
    UNIQUE (user_id, ledger_id),
    -- address must be unique per ledger_id basis, but otherwise is a sort
    -- of group address
    UNIQUE (address, ledger_id)
);

CREATE TABLE IF NOT EXISTS account_permission
(
    id INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES account(id),
    user_id INTEGER NOT NULL REFERENCES "user"(id),
    permissions INTEGER NOT NULL,
    updated_at DATETIME NOT NULL DEFAULT (datetime('now', 'subsec')),
    created_at DATETIME NOT NULL DEFAULT (datetime('now', 'subsec'))
);
CREATE TABLE IF NOT EXISTS transfer
(
    id INTEGER PRIMARY KEY,
    debit_account_id INTEGER NOT NULL REFERENCES account(id),
    credit_account_id INTEGER NOT NULL REFERENCES account(id),
    amount INTEGER NOT NULL,
    pending_id INTEGER REFERENCES transfer(id),
    ledger_id INTEGER NOT NULL REFERENCES ledger(id),
    code INTEGER NOT NULL,
    flags INTEGER NOT NULL,
    memo TEXT,
    created_at DATETIME NOT NULL DEFAULT (datetime('now', 'subsec'))
);

-- +goose Down
DROP TABLE IF EXISTS transfer;
DROP TABLE IF EXISTS account_permission;
DROP TABLE IF EXISTS account;
DROP TABLE IF EXISTS ledger;
