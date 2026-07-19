# Accounts

All routes under `/accounts/{account_id}` require an account token via the `Authorization` header. Replace `{account_id}` with the actual account ID.

## Routes

<details>
<summary><code>GET</code> <code><b>/accounts</b></code> <code>(search accounts by term and ledger)</code></summary>

##### Parameters
- Query params:
  - `term` (string, required) — search term to match against account address or username
  - `ledgerid` (string, required) — ledger ID, parsed to int64

##### Example
```bash
curl -X GET "https://stelo.finance/api/accounts?term=alice&ledgerid=1"
```

##### Responses
http code `200` | Content-Type `application/json`
```jsonc
[
  {
    "id": 42,                    // int64 — account ID
    "address": "alice",          // string — account address
    "bitcraftUsername": "alice"  // string|null — linked username
  }
]
```

http code `400` | Returned when `term` or `ledgerid` is missing or `ledgerid` is not a valid integer.

</details>

<details>
<summary><code>GET</code> <code><b>/accounts/{account_id}/ping</b></code> <code>(auth health check)</code></summary>

##### Parameters
No parameters required.

##### Example
```bash
curl -X GET https://stelo.finance/api/accounts/42/ping \
  -H "Authorization: <token>"
```

##### Responses
http code `200` | Content-Type `text/plain`
```
pong
```

</details>

<details>
<summary><code>GET</code> <code><b>/accounts/{account_id}</b></code> <code>(get account details)</code></summary>

##### Parameters
No parameters required.

##### Example
```bash
curl -X GET https://stelo.finance/api/accounts/42 \
  -H "Authorization: <token>"
```

##### Responses
http code `200` | Content-Type `application/json`
```jsonc
{
  "userId": 7,           // int64|null — linked user ID
  "balance": 300,        // int64 — computed account balance
  "debitsPending": 0,    // int64
  "debitsPosted": 500,   // int64
  "creditsPending": 0,   // int64
  "creditsPosted": 200,  // int64
  "ledgerId": 1,         // int64
  "code": 0,             // int64 — account code
  "createdAt": "2024-01-15T10:30:00Z"  // RFC 3339 string
}
```

</details>

<details>
<summary><code>GET</code> <code><b>/accounts/{account_id}/transfers</b></code> <code>(list account transfers)</code></summary>

##### Parameters
No parameters required.

##### Example
```bash
curl -X GET https://stelo.finance/api/accounts/42/transfers \
  -H "Authorization: <token>"
```

##### Responses
http code `200` | Content-Type `application/json`
```jsonc
[
  {
    "id": 99,                  // int64 — transfer ID
    "debitAccId": 42,          // int64 — debiting account ID
    "creditAccId": 7,          // int64 — crediting account ID
    "amount": 250,             // int64 — transfer amount
    "ledgerId": 1,             // int64
    "debitAddr": "ANSYZS",     // string — debit account address
    "creditAddr": "QHCJYZ"     // string — credit account address
    "code": 1,                 // int32 — transfer code
    "memo": "food payment",    // string|null — optional memo
    "createdAt": "2024-01-15T11:00:00Z"  // RFC 3339 string
  }
]
```

</details>

<details>
<summary><code>GET</code> <code><b>/accounts/{account_id}/transfers/{tr_id}</b></code> <code>(get a single transfer)</code></summary>

##### Parameters
- Path params:
  - `tr_id` (int64, required) — transfer ID

##### Example
```bash
curl -X GET https://stelo.finance/api/accounts/42/transfers/99 \
  -H "Authorization: <token>"
```

##### Responses
http code `200` | Content-Type `application/json`
```jsonc
{
  "id": 99,                  // int64 — transfer ID
  "debitAccId": 42,          // int64 — debiting account ID
  "creditAccId": 7,          // int64 — crediting account ID
  "amount": 250,             // int64 — transfer amount
  "ledgerId": 1,             // int64
  "debitAddr": "ANSYZS",     // string — debit account address
  "creditAddr": "QHCJYZ"     // string — credit account address
  "code": 1,                 // int32 — transfer code
  "memo": "food payment",    // string|null — optional memo
  "createdAt": "2024-01-15T11:00:00Z"  // RFC 3339 string
}
```

http code `404` | Returned when the transfer is not found.

</details>

<details>
<summary><code>POST</code> <code><b>/accounts/{account_id}/transfers</b></code> <code>(create a transfer)</code></summary>

##### Parameters
- Headers:
  - `Idempotency-Key` (string, required) — client-generated key (max 64 chars) unique per intended transfer for this account. Retries with the same key and same body return the original transfer; same key with a different body returns `409`.
- Body fields (JSON):
  - `receivingId` (int64, required) — destination account ID
  - `memo` (string, optional) — transfer memo
  - `ledgerId` (int64, required) — ledger ID
  - `amount` (int64, required) — amount to transfer, must be >= 1

##### Example
```bash
curl -X POST https://stelo.finance/api/accounts/42/transfers \
  -H "Authorization: <token>" \
  -H "Content-Type: application/json" \
  -H "Idempotency-Key: 550e8400-e29b-41d4-a716-446655440000" \
  -d '{"receivingId":7,"ledgerId":1,"amount":250,"memo":"payment"}'
```

##### Responses
http code `201` | Content-Type `application/json` — transfer created
```jsonc
{
  "id": 99,                  // int64 — transfer ID
  "debitAccId": 42,          // int64
  "creditAccId": 7,          // int64
  "amount": 250,             // int64
  "ledgerId": 1,             // int64
  "debitAddr": "ANSYZS",     // string
  "creditAddr": "QHCJYZ",    // string
  "code": 1,                 // int32
  "memo": "payment",         // string|null
  "createdAt": "2024-01-15T11:00:00Z"  // RFC 3339 string
}
```

http code `200` | Content-Type `application/json` — same body as `201`, returned when replaying a prior successful request with the same `Idempotency-Key` and payload.

http code `400` | Bad Request — missing/invalid idempotency key, invalid balance, or validation failure.

http code `409` | Conflict — `Idempotency-Key` was already used with a different request payload.

</details>
