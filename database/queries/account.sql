-- name: UpdateAccountBalances :execrows
UPDATE account
    SET debits_posted = debits_posted + sqlc.arg(debits_posted),
        debits_pending = debits_pending + sqlc.arg(debits_pending),
        credits_posted = credits_posted + sqlc.arg(credits_posted),
        credits_pending = credits_pending + sqlc.arg(credits_pending)
    WHERE id = sqlc.arg(account_id);

-- name: UpdateAccountBalance :execrows
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

-- name: UpdateDebitsPosted :execrows
UPDATE account
SET debits_posted = debits_posted + @quantity
WHERE id = @id
AND CASE
    WHEN code BETWEEN 0 AND 99 THEN credits_posted >= debits_pending + debits_posted + @quantity
    ELSE TRUE
END;

-- name: UpdateDebitsPending :execrows
UPDATE account
SET debits_pending = debits_pending + @quantity
WHERE id = @id
AND CASE
    WHEN code BETWEEN 0 AND 99 THEN credits_posted >= debits_pending + debits_posted + @quantity
    ELSE TRUE
END;

-- name: UpdateCreditsPosted :execrows
UPDATE account
SET credits_posted = credits_posted + @quantity
WHERE id = @id
AND CASE
    WHEN code BETWEEN 100 AND 199 THEN debits_posted >= credits_pending + credits_posted + @quantity
    ELSE TRUE
END;

-- name: UpdateCreditsPending :execrows
UPDATE account
SET credits_pending = credits_pending + @quantity
WHERE id = @id
AND CASE
    WHEN code BETWEEN 100 AND 199 THEN debits_posted >= credits_pending + credits_posted + @quantity
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

-- name: GetAccountWithUsernameById :one
SELECT
    a.*,
    u.bitcraft_username
FROM account AS a
LEFT JOIN "user" AS u ON u.id = a.user_id
WHERE a.id = ?;

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

-- name: UpdateAccountUserId :execrows
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

-- name: SearchAccountsByAddrAndUsername :many
SELECT
    a.id,
    a.address,
    u.bitcraft_username
FROM account AS a
LEFT JOIN "user" as u
    ON a.user_id = u.id
WHERE
    (a.address LIKE sqlc.arg(search_term) OR UPPER(u.bitcraft_username) LIKE sqlc.arg(search_term))
    AND a.id != sqlc.arg(exclude_account_id)
    AND a.ledger_id = sqlc.arg(ledger_id)
LIMIT sqlc.arg(limit);

-- name: LedgerBalanceAudit :one
SELECT
    SUM(CASE
        WHEN a.code BETWEEN 100 AND 199
        THEN a.debits_posted - a.credits_pending - a.credits_posted
        ELSE 0 END) AS debits_net,
    SUM(CASE WHEN a.code BETWEEN 0 AND 99
        THEN a.credits_posted - a.debits_pending - a.debits_posted
        ELSE 0 END) AS credits_net
FROM account a
WHERE ledger_id = ?;
