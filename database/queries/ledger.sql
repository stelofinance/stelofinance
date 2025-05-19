-- name: GetLedgerCodes :many
SELECT id, code FROM ledger WHERE id = ANY($1::BIGINT[]);
