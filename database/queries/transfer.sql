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

-- name: GetTransfersUserHasPermsOn :many
SELECT
	t.*,
	da.address as debit_addr,
	ca.address as credit_addr,
	du.bitcraft_username as debit_username,
	cu.bitcraft_username as credit_username,
	l.name as ledger_name,
	l.asset_scale
FROM transfer t
INNER JOIN ledger l ON l.id = t.ledger_id
INNER JOIN account a ON a.id = t.debit_account_id OR a.id = t.credit_account_id
INNER JOIN account da ON da.id = t.debit_account_id
INNER JOIN account ca ON ca.id = t.credit_account_id
LEFT JOIN "user" du ON du.id = da.user_id
LEFT JOIN "user" cu ON cu.id = ca.user_id
INNER JOIN account_permission ap ON ap.account_id = a.id
WHERE ap.user_id = ?
	AND (CAST(sqlc.narg('account_id') AS INTEGER) IS NULL
		OR t.debit_account_id = sqlc.narg('account_id')
		OR t.credit_account_id = sqlc.narg('account_id'))
ORDER BY datetime(t.created_at) DESC
LIMIT 25;
