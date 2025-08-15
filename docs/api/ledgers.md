# Ledgers
Stelo Finance has "ledgers" which define what asset or currency is being transferred.

## Types of Ledgers
Below is a list of ledgers that are currently supported on Stelo.

| id | name    | bitcraft id | description                 |
|----|---------|-------------|-----------------------------|
| 1  | stelo   |             | The Stelo Finance currency. |
| 2  | hexcoin | 1           | Hex Coin in BitCraft        |

## Ledger Codes
Ledgers have "codes" that define what type of asset they are.

| code | description                                      |
|------|--------------------------------------------------|
| 0    | Digital items that exist solely on Stelo Finance |
| 100  | "Item" in BitCraft                               |
| 200  | "Cargo" in BitCraft                              |

## Routes

<details>
<summary><code>GET</code> <code><b>/ledgers</b></code> <code>(gets all ledgers)</code></summary>

##### Responses

http code `200` | Content-Type `application/json`
```jsonc
  [
    {
      "id": 1, // Stelo's ID for this ledger
      "name": "stelo", // Stelo's name for this ledger
      "assetScale": 3, // Asset's "scale", 3 means it has 3 decimal places
      "code": 0, // Type of ledger
      "value": 1 // It's collateral value, measured in Stelo
    }
  ]
```

</details>
