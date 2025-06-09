-- name: GetWalletPermissions :one
SELECT
	wp.wallet_id,
	wp.permissions
FROM
	wallet_permission AS wp
JOIN
	wallet AS w ON w.id = wp.wallet_id
JOIN
	"user" AS u ON wp.user_id = u.id
WHERE u.id = $1 AND w.address = $2;

-- name: GetUsersOnWallet :many
SELECT
	u.id AS user_id,
	u.discord_username,
	wp.permissions
FROM
	wallet_permission AS wp
JOIN
	wallet AS w ON w.id = wp.wallet_id
JOIN
	"user" AS u ON wp.user_id = u.id
WHERE w.address = sqlc.arg(wallet_addr);

-- name: InsertWalletPermission :one
INSERT
    INTO wallet_permission (wallet_id, user_id, permissions, updated_at, created_at)
    VALUES ($1, $2, $3, $4, $5)
RETURNING id;
