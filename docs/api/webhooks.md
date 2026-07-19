# Webhooks

Any account can have a singular webhook set on it. If an account has a webhook set then any incoming or outgoing transfer is broadcast to that webhook.

## Delivery semantics

Webhooks are delivered **at least once**. Stelo enqueues each delivery on a durable queue and retries on failure (network errors or non-2xx HTTP responses), up to a maximum number of attempts.

Because of retries, your endpoint may receive the same transfer more than once. **Treat the transfer `id` field as the idempotency key** in your application and ignore or no-op duplicate deliveries for an `id` you have already processed.

Requests are `POST` with `Content-Type: application/json` and `User-Agent: Stelo-Webhooks/1.0`. Respond with a **2xx** status to acknowledge successful receipt; any other status (or a timeout/network failure) triggers a retry.

## Webhook Payload

Your server (specified by the URL you've set) will be sent a POST request with a body such as the following:

```jsonc
{
    "id": 123, // transfer ID — use as your idempotency key
    "debitAccId": 281,
    "creditAccId": 93,
    "amount": 28308,
    "ledgerId": 2,
    "code": 1, // This is the type of transfer
    "memo": "lorem was here", // May be null
    "createdAt": "2006-01-02T15:04:05.999999999Z07:00" // RFC3339Nano
}
```

## Routes

<details>
<summary><code>GET</code> <code><b>/accounts/{account_id}/webhook</b></code> <code>(retrieve the account webhook URL)</code></summary>

##### Example

```bash
curl -X GET https://stelo.finance/api/accounts/123/webhook \
  -H "Authorization: <token>"
```

##### Responses

http code `200` | Content-Type `application/json`
```jsonc
"https://example.com/webhook"
```

http code `200` | Content-Type `application/json`
```jsonc
null // no webhook is currently set
```

</details>

<details>
<summary><code>PUT</code> <code><b>/accounts/{account_id}/webhook</b></code> <code>(set or update the account webhook URL)</code></summary>

##### Parameters

| Parameter   | Type   | In    | Description                        |
|-------------|--------|-------|------------------------------------|
| webhook     | string | body  | A valid URL to receive webhooks    |

##### Example

```bash
curl -X PUT https://stelo.finance/api/accounts/123/webhook \
  -H "Authorization: <token>" \
  -H "Content-Type: application/json" \
  -d '{"webhook": "https://example.com/webhook"}'
```

##### Responses

http code `200` | Webhook updated

http code `400` | Webhook URL is invalid or request body is malformed

</details>

<details>
<summary><code>DELETE</code> <code><b>/accounts/{account_id}/webhook</b></code> <code>(remove the account webhook)</code></summary>

##### Example

```bash
curl -X DELETE https://stelo.finance/api/accounts/123/webhook \
  -H "Authorization: <token>"
```

##### Responses

http code `200` | Webhook removed

</details>
