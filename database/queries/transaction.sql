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

-- name: GetTxs
SELECT
    tx.id,
    du.discord_username AS debit_username,
    cu.discord_username AS credit_username,
    tx.status
FROM transaction tx
JOIN wallet dw ON dw.id = tx.debit_wallet_id
JOIN wallet cw ON cw.id = tx.credit_wallet_id
JOIN wallet_permission dwp ON dwp.wallet_id = dw.id
JOIN wallet_permission cwp ON cwp.wallet_id = cw.id
JOIN "user" du ON du.id = dwp.user_id
JOIN "user" cu ON cu.id = cwp.user_id
WHERE tx.code = 2 AND tx.status = 1;
