-- name: InsertTransfer :one
INSERT INTO transfer (transaction_id, debit_account_id, credit_account_id, amount, pending_id, ledger_id, code, flags, created_at)
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) returning id;
