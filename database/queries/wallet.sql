-- name: InsertWallet :one
INSERT
    INTO wallet (address, code, webhook, location, collateral_account_id, collateral_locked_account_id, collateral_percentage, created_at)
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
RETURNING id;

-- name: InsertWalletPermission :one
INSERT
    INTO wallet_permission (wallet_id, user_id, permissions, updated_at, created_at)
    VALUES ($1, $2, $3, $4, $5)
RETURNING id;

-- name: GetWallet :one
SELECT * FROM wallet WHERE id = $1;
