# Wallets

## Routes
All routes are prefixed with `/wallets/{addr}`, so if the docs say `/accounts` it's `/wallets/{addr}/accounts`.
The paramtere `{addr}` should be replaced with your wallet's address.

#### Accounts
<details>
<summary><code>GET</code> <code><b>/accounts</b></code> <code>(gets all accounts on your wallet)</code></summary>

##### Responses

http code `200` | Content-Type `application/json`
```jsonc
[
    {
        "assetName": "hexcoin",
        "balance": 40013,
        "ledgerId": 2,
        "code": 101
    },
    {
        "assetName": "stelo",
        "balance": 1234567,
        "ledgerId": 1,
        "code": 101
    }
]
```

</details>

---

#### Transactions
<details>
<summary><code>GET</code> <code><b>/transactions</b></code> <code>(gets recent transactions to/from your wallet)</code></summary>

> Note, only shows most recent 50 transactions (for now).

##### Responses

http code `200` | Content-Type `application/json`
```jsonc
[
    {
        "id": 239,
        "debitAddr": "HMWARNFGA",
        "creditAddr": "XAHMRRCEU",
        "code": 0,
        "memo": "Just a little memo", // optional
        "createdAt": "2025-08-14T13:37:25.518383-04:00", // RFC3339Nano
        "status": 0
    },
    {
        "id": 238,
        "debitAddr": "XAHMRRCEU",
        "creditAddr": "HMWARNFGA",
        "code": 0,
        "createdAt": "2025-08-14T13:36:41.443446-04:00",
        "status": 0
    }
]
```

</details>

<details>
<summary><code>GET</code> <code><b>/transactions/{tx_id}</b></code> <code>(get specific transaction)</code></summary>

##### Parameters

| name    | type | description                  |
|---------|------|------------------------------|
| `tx_id` | int  | Id of the transaction to get |

##### Responses

http code `200` | Content-Type `application/json`
```jsonc
{
    "id": 239,
    "debitAddress": "HMWARNFGA",
    "creditAddress": "XAHMRRCEU",
    "code": 0,
    "memo": "delete stelo test",
    "createdAt": "2025-08-14T13:37:25.518383-04:00",
    "status": 0
}
```

</details>

<details>
<summary><code>GET</code> <code><b>/transactions/{tx_id}/transfers</b></code> <code>(get all transfers on a transaction)</code></summary>

##### Parameters

| name    | type | description           |
|---------|------|-----------------------|
| `tx_id` | int  | Id of the transaction |

##### Responses

http code `200` | Content-Type `application/json`
```jsonc
[
    {
        "amount": 1,
        "ledgerId": 1,
        "createdAt": "2025-08-13T22:32:30.670365-04:00"
    }
]
```

</details>

<details>
<summary><code>POST</code> <code><b>/transactions</b></code> <code>(Create a transaction)</code></summary>

##### Body
```jsonc
{
    "receivingAddr": "HMWARNFGA", // Wallet address you want to send to
    "memo": "Ipsum was here", // (Optional)
    "transfers": [ // Array of account transfers you want to make (sending items)
        {
            "ledgerId": 1,
            "amount": 42
        }
    ]
}
```

##### Responses

http code `201`

</details>

---

#### Webhooks

<details>
<summary><code>GET</code> <code><b>/webhook</b></code> <code>(get webhook on this wallet)</code></summary>

##### Responses

http code `200` | Content-Type `application/json`
```json
"https://your-api.com/stelo-webhook?secret=123"
```

</details>

<details>
<summary><code>PUT</code> <code><b>/webhook</b></code> <code>(Update/add webhook to this wallet)</code></summary>

##### Body
```jsonc
{
    "webhook": "https://your-api.com/stelo-webhook?secret=123"
}
```

##### Responses

http code `200`

</details>

<details>
<summary><code>DELETE</code> <code><b>/webhook</b></code> <code>(Remove webhook from wallet)</code></summary>

##### Responses

http code `200`

</details>
