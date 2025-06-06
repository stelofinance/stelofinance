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

-- name: GetWalletAddr :one
SELECT address FROM wallet WHERE id = $1;

-- name: SearchWalletAddr :many
SELECT wallet.address
FROM wallet
WHERE wallet.address ILIKE $1
LIMIT $2;

-- name: SearchWalletAddrByDiscord :many
SELECT
	u.discord_username,
	w.address
FROM
	"user" AS u
JOIN wallet AS w ON w.id = u.wallet_id
WHERE u.discord_username ILIKE $1
LIMIT $2;

-- name: GetWalletIdsByAddr :many
SELECT id, address FROM wallet WHERE address = ANY($1::TEXT[]);

-- name: GetWalletsByLocation :many
SELECT
	w.address,
	ST_AsText(w.location)::TEXT AS warehouse_coordinates,
	ST_Distance($1, w.location)::INT AS distance
FROM
	wallet AS w
WHERE
	w.code = 200
ORDER BY
	distance
LIMIT $2;
