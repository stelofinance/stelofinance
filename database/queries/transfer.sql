-- name: InsertTransfer :one
INSERT INTO transfer (debit_account_id, credit_account_id, amount, pending_id, ledger_id, code, flags, memo, created_at)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) returning id;

-- name: GetTransfersByAccountId :many
SELECT
    tr.*,
    da.address AS debit_address,
    ca.address AS credit_address
FROM transfer AS tr
JOIN
	account AS da ON da.id = tr.debit_account_id
JOIN
	account AS ca ON ca.id = tr.credit_account_id
WHERE debit_account_id = sqlc.arg(account_id) OR credit_account_id = sqlc.arg(account_id)
LIMIT 250;

-- name: GetTransferById :one
SELECT * FROM transfer WHERE id = ?;

-- name: GetTransferWithAddrsById :one
SELECT
    tr.*,
    da.address AS debit_address,
    ca.address AS credit_address
FROM transfer AS tr
JOIN
	account AS da ON da.id = tr.debit_account_id
JOIN
	account AS ca ON ca.id = tr.credit_account_id
WHERE tr.id = ?;
