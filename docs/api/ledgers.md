# Ledgers
Stelo Finance has "ledgers" which define what asset or currency is being transferred.

## Ledger Scale
Ledgers may have a non-zero scale. If a ledger has a scale of 2 for example, then 100 qty in an account is actually 1.00 items.

A scale of 4 means that a balance of 43288 is presented as 4.3288.

## Ledger Codes
Ledgers have "codes" that define what type of asset they are.

| code | description                                                                  |
|------|------------------------------------------------------------------------------|
| 0    | Purely digital items, with no redemption in-game                             |
| 1    | A derivation item, meaning it is partially or in some way redeemable in-game |
| 100  | A regular item in BitCraft, fully redeemable 1:1 for the item in-game        |

## Routes

<details>
<summary><code>GET</code> <code><b>/ledgers</b></code> <code>(gets all ledgers)</code></summary>

##### Responses

http code `200` | Content-Type `application/json`
```jsonc
  [
    {
      "id": 1, // Stelo's internal ID for this ledger
      "name": "hexcoin", // Stelo's name for this ledger
      "assetScale": 3, // Asset's "scale", 3 means it has 3 decimal places
      "code": 0 // Type of ledger
    }
  ]
```

</details>
