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

