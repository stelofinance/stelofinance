-- name: InsertTransaction :one
INSERT INTO transaction (debit_wallet_id, credit_wallet_id, code, memo, created_at)
    VALUES ($1, $2, $3, $4, $5)
    RETURNING id;
