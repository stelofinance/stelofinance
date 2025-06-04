-- name: GetLedgers :many
SELECT * FROM ledger WHERE id = ANY($1::BIGINT[]);

-- name: GetLedger :one
SELECT * FROM ledger WHERE id = $1;
