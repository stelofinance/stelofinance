# Webhooks
Any account can have a singular webhook set on it. If an account has a webhook set than any incoming or outgoing transfer is broadcast to that webhook.

## Updating or Removing Webhooks
WIP

## Webhook POST Example
Your server (specified by the URL you've set) will be sent a POST request with a body such as the following: 

```jsonc
{
    "id": 123, // transfer ID
    "debitAccId": 281,
    "creditAccId": 93,
    "debitAddr": "RYALZU",
    "creditAddr":"ZYURLF",
    "amount": 28308,
    "ledgerId": 2,
    "code": 1, // This is the type of transfer
    "memo": "lorem was here", // May be null
    "createdAt": "2006-01-02T15:04:05.999999999Z07:00" // RFC3339Nano
}
```
