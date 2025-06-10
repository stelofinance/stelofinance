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

-- name: GetUserOnWallet :one
SELECT
	wp.permissions
FROM
	wallet_permission AS wp
JOIN
	"user" AS u ON wp.user_id = u.id
WHERE wp.wallet_id = sqlc.arg(wallet_id) AND u.discord_username = sqlc.arg(discord_username);

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

-- name: DeleteWalletPerm :exec
DELETE FROM wallet_permission AS wp
WHERE wp.wallet_id = sqlc.arg(wallet_id) AND user_id IN (
	SELECT id AS user_id
	FROM "user"
	WHERE discord_username = sqlc.arg(discord_username)
);

-- name: UpdateWalletPerm :exec
UPDATE wallet_permission wp
SET permissions = $1
FROM "user" u
WHERE wp.user_id = u.id
	AND wp.wallet_id = sqlc.arg(wallet_id)
	AND u.discord_username = sqlc.arg(discord_username);
