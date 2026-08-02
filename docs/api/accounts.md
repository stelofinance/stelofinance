# Accounts

Account-scoped routes use an **account token** via the `Authorization` header (raw secret only; no `Bearer ` prefix). The token binds to a single account — there is no account id path segment until SpacetimeDB supports route parameters.

## SpacetimeDB module HTTP (current)

Host prefix:

```text
$STDB_URI/v1/database/$DATABASE/route
```

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `GET` | `/ping` | none | Health check → `pong` |
| `GET` | `/accounts` | none | Public search by address / primary username |
| `GET` | `/account/ping` | token | Health check → `pong` |
| `GET` | `/account` | token | Account details |
| `GET` | `/account/transfers` | token | List transfers (`limit`, `offset`) |
| `POST` | `/account/transfers` | token | Create transfer (token account = **sender**) |
| `PUT` | `/account/transfer` | token | Finalize pending (post / partial / void) |

### `GET /accounts` (public search)

No auth. Find accounts on a ledger by address or primary username substring (case-insensitive).

Query params:

| Param | Required | Notes |
|-------|----------|--------|
| `term` | yes | Non-empty; matched as substring after uppercasing |
| `ledgerid` | yes | Ledger id (`ledgerId` accepted as alias) |
| `limit` | no | Default `10`, max `50` |

```bash
curl -s "$STDB/v1/database/stelofinance/route/accounts?term=alice&ledgerid=1"
```

```jsonc
[
  {
    "id": 42,
    "address": "alice",
    "bitcraftUsername": "alice"   // null if account has no primary user
  }
]
```

`400` if `term` or `ledgerid` is missing/invalid, or ledger does not exist.

### `GET /account`

```bash
curl -s "$STDB/v1/database/stelofinance/route/account" \
  -H "Authorization: <token>"
```

```jsonc
{
  "id": 42,
  "address": "ANSYZS",
  "kind": "Debit",           // "Debit" | "Credit"
  "balance": 300,
  "debitsPending": 0,
  "debitsPosted": 500,
  "creditsPending": 0,
  "creditsPosted": 200,
  "ledgerId": 1,
  "isPrimary": true,
  "createdAt": "2024-01-15T10:30:00Z"
}
```

### `GET /account/transfers`

Query params:

- `limit` — default `100`, max `1000`
- `offset` — default `0`

Order: newest first (`createdAt` desc, then `id` desc).

```bash
curl -s "$STDB/v1/database/stelofinance/route/account/transfers?limit=50&offset=0" \
  -H "Authorization: <token>"
```

```jsonc
[
  {
    "id": 99,
    "debitAccId": 42,
    "creditAccId": 7,
    "pendingAmount": null,
    "postedAmount": 250,
    "amount": 250,              // pending if Pending, else posted
    "ledgerId": 1,
    "debitAddr": "ANSYZS",
    "creditAddr": "QHCJYZ",
    "kind": "Asset",            // Liability | Asset | Issue | Redeem
    "state": "Posted",          // Posted | Pending | PostPending | VoidPending
    "memo": "food payment",
    "createdAt": "2024-01-15T11:00:00Z",
    "finalizedAt": "2024-01-15T11:00:00Z"
  }
]
```

### `POST /account/transfers`

Headers:

- `Idempotency-Key` (required, max 64 chars)
- `Authorization`, `Content-Type: application/json`

Body:

```jsonc
{
  "receivingId": 7,
  "amount": 250,
  "memo": "payment",      // optional
  "pending": false        // optional, default false
}
```

Status: `201` created, `200` idempotent replay, `409` key conflict, `400` validation.

### `PUT /account/transfer`

Finalize a pending transfer. Singular path is temporary until path params exist (`/account/transfers/{id}`).

```jsonc
{
  "transferId": 99,
  "amount": 0             // 0 = void; 1..=held = post or partial post
}
```

Status: `200` + transfer JSON; `403` if token is not the authorized leg; `404` not found; `400` validation.

Only **Issue** and **Redeem** pending transfers can be finalized (same as module reducer).

---

## Legacy Go edge (`https://stelo.finance/api`) — reference

The historical edge used `/accounts/{account_id}/…` and integer `code` fields. Prefer the module HTTP surface above for new integrations. Webhook URL CRUD remains UI/reducer-only (not on module HTTP).
