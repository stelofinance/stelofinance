-- name: GetLedgers :many
SELECT * FROM ledger WHERE id = ANY($1::BIGINT[]);
