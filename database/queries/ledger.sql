-- name: InsertLedger :one
INSERT INTO ledger (name, asset_scale, code) VALUES (?, ?, ?) RETURNING id;

-- name: GetLedgers :many
SELECT * FROM ledger WHERE id IN (sqlc.slice('ids'));

-- name: GetAllLedgers :many
SELECT * FROM ledger;

-- name: GetLedger :one
SELECT * FROM ledger WHERE id = ?;

-- name: GetLedgersByCode :many
SELECT * FROM ledger WHERE code = ?;
