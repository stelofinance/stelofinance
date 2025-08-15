# Webhooks
Any wallet can have a singular webhook set on it. If a wallet has a webhook set than any incoming transaction is broadcast to that webhook.

## Updating or Removing Webhooks
Please refer to the [Webhook routes](./wallets.md#webhooks) in Wallet Routes for documentation on how to add/update or remove a webhook from a wallet.

## Webhook POST Example
Your server (specified by the URL you've set) will be sent a POST request with a body such as the following: 

```jsonc
{
    "id": 123, // Transaction ID
    "debitAddr": "XYZABC",
    "creditAddr": "ABCXYZ",
    "code": 1, // This is the "purpose" of the tx
    "memo": "lorem was here", // May be omitted
    "createdAt": "2006-01-02T15:04:05.999999999Z07:00" // RFC3339Nano
    "status": 0, // posted, pending, post_pending (0, 1, 2) 
    "transfers": [
        {
            "ledgerId": 1,
            "amount": 42
        },
        {
            "ledgerId": 2,
            "amount":
        }
    ]
}
```
