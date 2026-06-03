# Auth

## Account Token

Account token auth is handled via the `Authorization` header.

| Header          | Value        |
|-----------------|--------------|
| Authorization   | `<token>`    |

Used by all routes under `/accounts/{account_id}/*` (e.g. transfers, webhooks, account info, ping).

Create an account token in your account settings on the app website. This token will have admin access to that specific account, so be careful with it.
