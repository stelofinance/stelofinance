-- name: InsertTransfer :one
INSERT INTO transfer (transaction_id, debit_account_id, credit_account_id, amount, pending_id, ledger_id, code, flags, created_at)
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) returning id;

-- name: GetTransfersByTxId :many
SELECT * FROM transfer WHERE transaction_id = $1;

-- name: InsertTransfers :copyfrom
INSERT INTO transfer (transaction_id, debit_account_id, credit_account_id, amount, pending_id, ledger_id, code, flags, created_at)
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9);

-- name: GetTransfersAssetsByTxIds :many
SELECT
    t.transaction_id,
    t.amount,
    l.name,
    l.asset_scale,
    l.code
FROM transfer t
JOIN ledger l ON l.id = t.ledger_id
WHERE t.transaction_id = ANY(sqlc.arg(transaction_id)::BIGINT[]);
