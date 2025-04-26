-- name: UpdateDebitAccountDebitsPosted :one
UPDATE account
    SET debits_posted = debits_posted + $1
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
        AND debits_posted >= credits_posted + $1 -- credits must not exceed debits
    RETURNING id;

-- name: UpdateCreditAccountDebitsPosted :one
UPDATE account
    SET debits_posted = debits_posted + $1
    WHERE wallet_id = $2
        AND code = $3
        AND ledger_id = $4
        AND credits_posted >= debits_posted + $1 -- debits must not exceed credits
    RETURNING id;

-- name: UpdateCreditAccountCreditsPosted :one
UPDATE account
    SET credits_posted = credits_posted + $1
    WHERE wallet_id = $2
        AND code = $3
        AND ledger_id = $4
    RETURNING id;

-- name: InsertAccount :one
INSERT
    INTO account (wallet_id, debits_pending, debits_posted, credits_pending, credits_posted, ledger_id, code, flags, created_at)
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
    RETURNING id;
