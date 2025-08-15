# Auth
Auth is handled on Stelo in the `Authorization` header.

| Auth Type | Description                                                                                                                                                                                                                                       |
| --------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Wallet    | Create a wallet token in your app dashboard `stelo.finance/app/wallets/{WALLET_ADDR}/settings`. This token will have admin access to your entire wallet, so be careful with it. Attach this token to your request via the `Authorization` header. |
