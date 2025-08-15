-- name: UpdateAccountBalances :exec
UPDATE account
    SET debits_posted = debits_posted + $1,
        debits_pending = debits_pending + $2,
        credits_posted = credits_posted + $3,
        credits_pending = credits_pending + $4
    WHERE id = $5;

-- name: UpdateAccountBalance :one
UPDATE account
SET 
    debits_posted = CASE WHEN sqlc.arg(field)::TEXT = 'debits_posted' THEN debits_posted + sqlc.arg(quantity) ELSE debits_posted END,
    debits_pending = CASE WHEN sqlc.arg(field) = 'debits_pending' THEN debits_pending + sqlc.arg(quantity) ELSE debits_pending END,
    credits_posted = CASE WHEN sqlc.arg(field) = 'credits_posted' THEN credits_posted + sqlc.arg(quantity) ELSE credits_posted END,
    credits_pending = CASE WHEN sqlc.arg(field) = 'credits_pending' THEN credits_pending + sqlc.arg(quantity) ELSE credits_pending END
WHERE wallet_id = $1
    AND code = $2
    AND ledger_id = $3
    AND CASE
        WHEN ($2 BETWEEN 100 AND 199 OR $2 = 201 OR $2 = 202) AND sqlc.arg(field) IN ('credits_posted', 'credits_pending') THEN (debits_posted >= credits_pending + credits_posted + sqlc.arg(quantity))
        WHEN ($2 = 0 OR $2 = 200) AND sqlc.arg(field) IN ('debits_posted', 'debits_pending') THEN (credits_posted >= debits_pending + debits_posted + sqlc.arg(quantity))
        ELSE TRUE
    END
RETURNING id;

-- name: InsertAccount :one
INSERT
    INTO account (wallet_id, debits_pending, debits_posted, credits_pending, credits_posted, ledger_id, code, flags, created_at)
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
    RETURNING id;

-- name: InsertAccountWithBalance :one
INSERT
    INTO account (wallet_id, debits_pending, debits_posted, credits_pending, credits_posted, ledger_id, code, flags, created_at)
    VALUES (
        $1,
        CASE WHEN sqlc.arg(field)::TEXT = 'debits_pending' THEN sqlc.arg(quantity) ELSE 0 END,
        CASE WHEN sqlc.arg(field)::TEXT = 'debits_posted' THEN sqlc.arg(quantity) ELSE 0 END,
        CASE WHEN sqlc.arg(field)::TEXT = 'credits_pending' THEN sqlc.arg(quantity) ELSE 0 END,
        CASE WHEN sqlc.arg(field)::TEXT = 'credits_posted' THEN sqlc.arg(quantity) ELSE 0 END,
        $2,
        $3,
        $4,
        $5
    ) RETURNING id;

-- name: GetAccountByWalletAddrAndLedgerName :one
SELECT
    l.asset_scale,
    l.code AS ledger_code,
    a.code AS account_code,
    a.debits_pending,
    a.debits_posted,
    a.credits_pending,
    a.credits_posted
FROM
    wallet AS w
JOIN
    account AS a ON a.wallet_id = w.id
JOIN
    ledger AS l ON l.id = a.ledger_id
WHERE
    w.address = $1 AND l.name = $2
LIMIT 1;

-- name: GetAccountBalancesByWalletAddr :many
SELECT
    w.id AS wallet_id,
    l.id AS ledger_id,
    l.name AS asset_name,
    (a.debits_posted - a.credits_posted - a.credits_pending)::BIGINT AS debit_balance,
    (a.credits_posted - a.debits_posted - a.debits_pending)::BIGINT AS credit_balance,
    l.asset_scale,
    a.code
FROM
    account AS a
JOIN ledger AS l ON a.ledger_id = l.id
JOIN wallet AS w ON w.id = a.wallet_id
WHERE w.address = $1;

-- name: GetAccountBalancesByWalletIdAndCode :many
SELECT
    l.id AS ledger_id,
    l.name AS asset_name,
    (a.debits_posted - a.credits_posted - a.credits_pending)::BIGINT AS debit_balance,
    (a.credits_posted - a.debits_posted - a.debits_pending)::BIGINT AS credit_balance,
    l.asset_scale,
    a.code
FROM account a
JOIN ledger l ON a.ledger_id = l.id
WHERE a.wallet_id = sqlc.arg(wallet_id) AND a.code = ANY(sqlc.arg(codes)::INT[]);
