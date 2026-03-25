-- name: GetAccountPermissions :one
SELECT permissions
FROM account_permission
WHERE user_id = ? AND account_id = ?;

-- name: GetUserOnAccount :one
SELECT
	ap.permissions
FROM
	account_permission AS ap
JOIN
	"user" AS u ON ap.user_id = u.id
WHERE ap.account_id = sqlc.arg(account_id) AND u.bitcraft_username = sqlc.arg(bitcraft_username);

-- name: GetUsersOnAccount :many
SELECT
	ap.id,
	ap.user_id,
	u.bitcraft_username,
	ap.permissions
FROM
	account_permission AS ap
JOIN
	"user" AS u ON ap.user_id = u.id
WHERE ap.account_id = sqlc.arg(account_id);

-- name: InsertAccountPerm :one
INSERT
    INTO account_permission (account_id, user_id, permissions, updated_at, created_at)
    VALUES (?, ?, ?, ?, ?)
RETURNING id;

-- name: DeleteAccountPerm :exec
DELETE FROM account_permission AS ap
WHERE ap.account_id = sqlc.arg(account_id) AND user_id IN (
	SELECT id AS user_id
	FROM "user"
	WHERE bitcraft_username = sqlc.arg(bitcraft_username)
);

-- UPDATE FROM is broken in SQLC, once this PR is merged, this can be fixed
-- https://github.com/sqlc-dev/sqlc/pull/3610

-- name: UpdateAccountPerm :exec
UPDATE account_permission
SET permissions = ?
WHERE id = ?;
