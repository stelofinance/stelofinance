-- name: UpdateAccountBalances :exec
UPDATE account
    SET debits_posted = debits_posted + $1,
        debits_pending = debits_pending + $2,
        credits_posted = credits_posted + $3,
        credits_pending = credits_pending + $4
    WHERE id = $5;

-- name: UpdateDebitAccountDebitsPosted :one
UPDATE account
    SET debits_posted = debits_posted + $1
    WHERE wallet_id = $2
        AND code = $3
        AND ledger_id = $4
    RETURNING id;

-- name: UpdateDebitAccountDebitsPending :one
UPDATE account
    SET debits_pending = debits_pending + $1
    WHERE wallet_id = $2
        AND code = $3
        AND ledger_id = $4
    RETURNING id;

-- name: UpdateDebitAccountCreditsPosted :one
UPDATE account
    SET credits_posted = credits_posted + $1
    WHERE wallet_id = $2
        AND code = $3
        AND ledger_id = $4
        AND debits_posted >= credits_pending + credits_posted + $1 -- credits must not exceed debits
    RETURNING id;

-- name: UpdateDebitAccountCreditsPending :one
UPDATE account
    SET credits_pending = credits_pending + $1
    WHERE wallet_id = $2
        AND code = $3
        AND ledger_id = $4
        AND debits_posted >= credits_pending + credits_posted + $1 -- credits must not exceed debits
    RETURNING id;

-- name: UpdateCreditAccountDebitsPosted :one
UPDATE account
    SET debits_posted = debits_posted + $1
    WHERE wallet_id = $2
        AND code = $3
        AND ledger_id = $4
        AND credits_posted >= debits_pending + debits_posted + $1 -- debits must not exceed credits
    RETURNING id;

-- name: UpdateCreditAccountDebitsPending :one
UPDATE account
    SET debits_pending = debits_pending + $1
    WHERE wallet_id = $2
        AND code = $3
        AND ledger_id = $4
        AND credits_posted >= debits_pending + debits_posted + $1 -- debits must not exceed credits
    RETURNING id;

-- name: UpdateCreditAccountCreditsPosted :one
UPDATE account
    SET credits_posted = credits_posted + $1
    WHERE wallet_id = $2
        AND code = $3
        AND ledger_id = $4
    RETURNING id;

-- name: UpdateCreditAccountCreditsPending :one
UPDATE account
    SET credits_pending = credits_pending + $1
    WHERE wallet_id = $2
        AND code = $3
        AND ledger_id = $4
    RETURNING id;

-- name: InsertAccount :one
INSERT
    INTO account (wallet_id, debits_pending, debits_posted, credits_pending, credits_posted, ledger_id, code, flags, created_at)
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
    RETURNING id;

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
