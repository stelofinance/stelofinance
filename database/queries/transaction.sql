-- name: InsertTransaction :one
INSERT INTO transaction (debit_wallet_id, credit_wallet_id, code, memo, status, created_at)
    VALUES ($1, $2, $3, $4, $5, $6)
    RETURNING id;

-- name: GetTransaction :one
SELECT * FROM transaction WHERE id = $1;

-- name: UpdateTransactionStatus :exec
UPDATE transaction SET status = sqlc.arg(new_status) WHERE id = $1 AND status = sqlc.arg(current_status);
