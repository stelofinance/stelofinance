-- name: InsertTransaction :one
INSERT INTO transaction (debit_wallet_id, credit_wallet_id, code, memo, status, created_at)
    VALUES ($1, $2, $3, $4, $5, $6)
    RETURNING id;

-- name: GetTransaction :one
SELECT * FROM transaction WHERE id = $1;

-- name: UpdateTransactionStatus :exec
UPDATE transaction SET status = sqlc.arg(new_status) WHERE id = $1 AND status = sqlc.arg(current_status);

-- name: GetTransactionsByWalletId :many
SELECT 
    t.id,
    t.debit_wallet_id,
    t.credit_wallet_id,
    t.code,
    t.memo,
    t.created_at,
    t.status,
    dw.address AS debit_address,
    cw.address AS credit_address
FROM transaction AS t
JOIN wallet AS dw ON dw.id = t.debit_wallet_id
JOIN wallet AS cw ON cw.id = t.credit_wallet_id
WHERE t.debit_wallet_id = $1 OR t.credit_wallet_id = $1
ORDER BY t.created_at DESC
LIMIT $2;
