-- name: UpdateAccountBalances :exec
UPDATE account
    SET debits_posted = debits_posted + sqlc.arg(debits_posted),
        debits_pending = debits_pending + sqlc.arg(debits_pending),
        credits_posted = credits_posted + sqlc.arg(credits_posted),
        credits_pending = credits_pending + sqlc.arg(credits_pending)
    WHERE id = sqlc.arg(account_id);

-- name: UpdateAccountBalance :exec
-- SQLC doesn't recognize @field in the CASE, so I manually made it ?1 to match
UPDATE account
SET 
    debits_posted = CASE WHEN CAST(@field AS TEXT) = 'debits_posted' THEN debits_posted + @quantity ELSE debits_posted END,
    debits_pending = CASE WHEN @field = 'debits_pending' THEN debits_pending + @quantity ELSE debits_pending END,
    credits_posted = CASE WHEN @field = 'credits_posted' THEN credits_posted + @quantity ELSE credits_posted END,
    credits_pending = CASE WHEN @field = 'credits_pending' THEN credits_pending + @quantity ELSE credits_pending END
WHERE id = @id
AND CASE
    WHEN (code BETWEEN 100 AND 199) AND ?1 IN ('credits_posted', 'credits_pending') THEN (debits_posted >= credits_pending + credits_posted + @quantity)
    WHEN (code BETWEEN 0 AND 99) AND ?1 IN ('debits_posted', 'debits_pending') THEN (credits_posted >= debits_pending + debits_posted + @quantity)
    ELSE TRUE
END;

-- name: InsertAccount :one
INSERT
    INTO account (address, webhook, user_id, debits_pending, debits_posted, credits_pending, credits_posted, ledger_id, code, flags, created_at)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    RETURNING id;

-- name: InsertAccountWithBalance :one
INSERT
    INTO account (address, webhook, user_id, debits_pending, debits_posted, credits_pending, credits_posted, ledger_id, code, flags, created_at)
    VALUES (
        ?,
        ?,
        ?,
        CASE WHEN CAST(sqlc.arg(field) AS TEXT) = 'debits_pending' THEN CAST(sqlc.arg(quantity) AS INTEGER) ELSE 0 END,
        CASE WHEN sqlc.arg(field) = 'debits_posted' THEN sqlc.arg(quantity) ELSE 0 END,
        CASE WHEN sqlc.arg(field) = 'credits_pending' THEN sqlc.arg(quantity) ELSE 0 END,
        CASE WHEN sqlc.arg(field) = 'credits_posted' THEN sqlc.arg(quantity) ELSE 0 END,
        ?,
        ?,
        ?,
        ?
    ) RETURNING id;

-- name: GetAccountById :one
SELECT * FROM account WHERE id = ?;

-- name: GetAccountAndLedgerById :one
SELECT
    a.*,
    l.name AS ledger_name,
    l.asset_scale,
    l.code AS ledger_code
FROM account a
JOIN ledger l ON l.id = a.ledger_id
WHERE a.id = ?;

-- name: GetAccountByAddrAndLedgerId :one
SELECT * FROM account WHERE address = ? AND ledger_id = ?;

-- name: UpdateAccountWebhookById :exec
UPDATE account
SET webhook = ?
WHERE id = ?;

-- name: UpdateAccountUserId :exec
UPDATE account
SET user_id = ?
WHERE id = ?;

-- name: GetAccountsUserHasPerms :many
SELECT
    a.id,
    a.address,

    a.debits_pending,
    a.debits_posted,
    a.credits_pending,
    a.credits_posted,

    a.user_id AS primary_user_id,
    l.name AS ledger_name,
    l.asset_scale,
    l.code AS ledger_code,
    a.code AS account_code,
    ap.permissions
FROM account a
INNER JOIN ledger l ON l.id = a.ledger_id
INNER JOIN account_permission ap ON ap.account_id = a.id
WHERE ap.user_id = ?;
