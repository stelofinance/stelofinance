# Payment Requests

You can send pre-filled payment request URLs to other users. When a user opens the link, the payment form is pre-populated with the details you specify.

## Request URL

```
https://stelo.finance/app/request
```

## Query Parameters

- `ledgerid` (integer, required) — The ID of the ledger the payment is for.
- `recipientid` (integer, required) — The ID of the account receiving the payment.
- `amount` (integer, required) — The payment amount, in the ledger's base unit.
- `memo` (string, optional) — A note to attach to the payment request.

## Example

```
https://stelo.finance/app/request?ledgerid=1&recipientid=42&amount=5000&memo=Invoice%20%23123
```

## Pre-filled Page

When a user opens a valid payment request URL, the page loads with the ledger, recipient, amount, and memo (if provided) already filled in. The user then selects their sending account and confirms the payment.
