# App surface parity (Go HTML → Topcoat)

**Status:** Working inventory for edge app port  
**Date:** 2026-08-08  
**Related:** [spacetimedb-refactor.md](./spacetimedb-refactor.md) (§8.7 H*, §8.8, §9–§10)  
**Source:** Live Go routes (`internal/routes/routes.go`), handlers (`internal/handlers/app.go`, `handlers.go`), templates (`web/templates/pages/*`, chrome components).

This document is the **hard feature-parity floor** for browser HTML surfaces. Redesign nav, headers, and layout freely; preserve the **capabilities** listed here unless this doc or the refactor doc is updated.

**Out of scope here:** third-party JSON API and module HTTP reverse-proxy. Those are **required cutover work** but not Datastar pages — track them in the refactor doc (§8.7 I*, §9). See [Non-HTML surfaces](#non-html-surfaces) below for a short pointer only.

---

## Legend

| Tag | Meaning |
|-----|---------|
| **Content** | What’s rendered / data loaded |
| **Actions** | Mutations / navigation the user can do |
| **Reactive** | Datastar / live SSE / signals that already work in Go |
| **Gaps** | TODOs, half-wired, or domain exists but no HTML |

**Chrome (all `/app/*` in Go):** sticky app header (logo → `/app`, username) + fixed bottom menu (Home, Accounts, Transfer). Market tab is **commented out**. Logout exists as a route but is **not** linked in chrome.

**Auth gate:** all `/app/*` require session (`AuthUser`); account admin mutation routes also need **Admin** on that account.

**Design inventory IDs:** H1–H5 map to §8.7 of the refactor doc; D2–D3 are public pages.

---

## Compact parity checklist

| # | Surface | Must have (capabilities) | Live data? | Design ID |
|---|---------|--------------------------|------------|-----------|
| 1 | Marketing home | Hero, how-it-works, assets, contribute; Log In vs Dashboard | No | D2 |
| 2 | Login (BitAuth) | Sign in, return to app | No | D3 |
| 3 | Logout | Clear session → home | No | H5 |
| 4 | App home | Something for logged-in user (Go: greeting only) | Optional | — |
| 5 | Accounts list | List wallets + balances; create debit; open detail | **Yes** (balances) | H1 |
| 6 | Account admin | Primary; members add/remove; API tokens mint/revoke | Partial (action patches only) | H2 |
| 7 | Transfers | Filter account; history; send + recipient search + memo + idempotency | **Yes** (history/bal) | H3 |
| 8 | Payment request | Query-prefilled pay flow | No (one-shot submit) | H4 |

**Chrome capability (Topcoat Option A, 2026-08-08):**  
Desktop/tablet top: logo→`/app` · Accounts · Activity · Transfer▾ (Send / Deposit / Withdraw) · username→`/app/me`.  
Mobile top: logo + username only; bottom: Accounts · Activity · Transfer (send).  
Logout lives on `/app/me` (profile page — more content planned).

---

## Public / auth pages

### 1. Marketing home — `GET /`

**Design:** D2 · **Topcoat:** done (rough)

| | |
|--|--|
| **Content** | Hero, How Stelo Works, assets on platform, contribute; CTA Log In vs Dashboard from auth cookie |
| **Actions** | → `/login` or `/app`; external Discord/GitHub |
| **Reactive** | None |
| **Gaps** | None for parity |

### 2. Login — `GET /login`

**Design:** D3 · **Topcoat:** BitAuth-only (BitJita dropped)

| | |
|--|--|
| **Content** | Login CTA + beta notice |
| **Actions** | Start BitAuth OIDC (old Go: BitJita chat code) |
| **Reactive (old Go)** | Datastar poll for BitJita code → redirect to `/auth/{key}` |
| **Gaps for new edge** | BitJita path **not** ported (intentional). BitAuth UX polish only. |

### 3. BitJita auth complete — `GET /auth/{key}`

**Drop** (BitJita). Replaced by BitAuth callback on Topcoat (`/callback` / `/auth/bitauth/*`).

### 4. Logout — `GET /app/logout` (Go)

**Design:** H5 · **Topcoat:** BitAuth logout under `/auth/bitauth/logout` (or equivalent)

| | |
|--|--|
| **Content** | None (redirect) |
| **Actions** | Clear session/cookies → `/` |
| **Reactive** | None |
| **Gaps** | Not linked from app chrome in Go either — wire into redesigned chrome |

---

## App pages

### 5. App home — `GET /app`

**Template:** `app-home.html.tmpl` · **Shell active:** `home`

| | |
|--|--|
| **Content** | Welcome line with BitCraft username only |
| **Actions** | None |
| **Reactive** | None |
| **Gaps** | No balances, activity, or shortcuts. Thin page — redesign opportunity; low Go surface area. |

---

### 6. Accounts list — `GET /app/accounts`

**Design:** H1 · **Template:** `app-accounts.html.tmpl` · **Shell:** `accounts`

| | |
|--|--|
| **Content** | User’s accounts: address, ledger type badge (item), primary badge, ledger name, **formatted balance**; credit-ish accounts left border; ledger list for create form |
| **Actions** | Navigate → account detail; **Create Account** (pick ledger → debit wallet) |
| **Reactive** | `data-init="@get('/app/accounts/updates')"` — long-lived SSE; NATS `accounts.transfers.{id}.*` / `*.{id}` → full `#page-content` re-render (balances update on transfer). Create → Datastar **PatchElements** of full list. Client signals: `$creatingAcc` |
| **Gaps** | Create only **debit (GA)**; no credit/custom address in UI. No live update if *membership* changes while open. **STDB target:** `my_accounts` + `create_account` + view subscribe (not NATS). |

**Related routes (not separate pages):**

| Route | Role |
|-------|------|
| `GET /app/accounts/updates` | SSE stream for list patches |
| `POST /app/accounts` | Create debit account → patch list |

---

### 7. Account admin — `GET /app/accounts/{account_id}`

**Design:** H2 · **Template:** `app-account.html.tmpl` · **Shell:** `account` · **Authz:** Admin on account

| | |
|--|--|
| **Content** | Breadcrumb `#addr-ledger`; if admin: **Primary** toggle; **Permissions** user list; **Tokens** total count + one-time secret after mint |
| **Actions** | Set/clear primary (`PUT .../user-id`); add user by username (`POST .../users`); remove user (`DELETE .../users/{id}`); create token (`POST .../tokens`); revoke all tokens (`DELETE .../tokens`) |
| **Reactive** | Actions return Datastar **PatchElements** of full page content. Signals: `$primary`, `$addingUser`, `$addUsername`, `$token` (after create) |
| **Gaps** | • Template TODO: **per-permission roles** (only coarse admin gate; no edit perms) • **No balance / address display** beyond title • **No webhook URL CRUD** (API-only in Go; module has `set_account_webhook`) • **No apps / tickets** (module has apps; no HTML) • **No live updates** if another tab changes members/tokens • Token list is **count only**, not per-token labels/ids (module view has metadata) • Revoke is **all** only (module can revoke by ids) |

**Related routes:**

| Route | Role |
|-------|------|
| `PUT /app/accounts/{id}/user-id` | Primary on/off (signal `$primary`) |
| `POST /app/accounts/{id}/users` | Grant member (username → user) |
| `DELETE /app/accounts/{id}/users/{user_id}` | Revoke member |
| `POST /app/accounts/{id}/tokens` | Mint API token (show secret once) |
| `DELETE /app/accounts/{id}/tokens` | Revoke all tokens |

---

### 8. Transfers — `GET /app/transfers`

**Design:** H3 · **Template:** `app-transfers.html.tmpl` · **Shell:** `transfers`

| | |
|--|--|
| **Content** | Account filter select (“All Accounts” + `#addr/ledger`); if one account selected: **Send** form (recipient, qty, memo, idempotency key, balance label); **Transfers list** (direction, relative time, from→to, amount+ledger, memo) |
| **Actions** | Change selected account; search/select recipient; submit transfer; “Start Over” after send |
| **Reactive** | `data-init` + `change` → `GET /app/transfers/updates` (SSE; NATS scoped to selected or all accounts); filter change with `Send-Initial-State` reloads list+form; recipient field: debounced search + radio select → `GET /app/transfers/form-recipient` patches fieldset; submit → `POST /app/accounts/{id}/transfers` → signal `$sentMessage`; indicators `$sending`. Signals: `$accId`, `$recipientSearch`, `$recipientAccId`, `$sentMessage`, `$sending` |
| **Gaps** | • Template comment “TODO: Add in transfer section” is **stale** — send form exists when account selected • **No send when “All Accounts”** • **No pending / finalize UI** (module has pending + `finalize_transfer`; Go prod was posted-only) • **No transfer detail page** • Balance max on qty input **commented out** • No error toasts (status codes only) • After send, success is signal-only; list relies on NATS/SSE for new row • Idempotency “Start Over” re-fetches for new key |

**Related routes:**

| Route | Role |
|-------|------|
| `GET /app/transfers/updates` | SSE live list (+ initial state headers) |
| `GET /app/transfers/form-recipient` | Datastar partial: recipient fieldset (search / pick / clear) |
| `POST /app/accounts/{account_id}/transfers` | Create transfer (Admin+); `$sentMessage` |

---

### 9. Payment request — `GET /app/request?...`

**Design:** H4 · **Template:** `app-request.html.tmpl` · **Shell:** `request`  
**Query:** `ledgerid`, `recipientid`, `amount` required; `memo` optional (see `docs/payment-requests.md`)

| | |
|--|--|
| **Content** | Prefilled amount/ledger/recipient/memo; sender account select (primary first + other debit accounts on that ledger) |
| **Actions** | Choose from-account → SEND (create transfer to fixed recipient) |
| **Reactive** | Submit → Datastar `$sentMessage`; `$sending` indicator; `$accId` bind for path |
| **Gaps** | No live balance on selected from-account; no link builder in-app (external URL only); bad query → bare 400; not in bottom nav |

**Related:** `POST /app/request/{account_id}/transfers` (Admin+ on sender)

---

## Shell / shared UI (not full pages)

| Piece | Capability to preserve |
|-------|------------------------|
| **App nav** | Logo home, show username |
| **App menu** | Nav to Home / Accounts / Transfers; active highlight |
| **Public nav/footer** | Marketing only (ported with index) |
| **Transfer recipient partial** | Search → suggestions → selected chip + clear |
| **Idempotency keys** | Generated server-side on form render (uuid) for transfer + payment request |
| **Display formatting** | Asset scale → human qty; relative times on transfers |
| **Hot reload** | Dev-only in Go (`GET /hotreload`); Topcoat: `topcoat dev` |

---

## Domain present but no Go HTML

Track so redesign can optionally exceed Go UI without forgetting product capabilities:

| Capability | Go UI | Module / notes |
|------------|-------|----------------|
| Webhook URL set/clear | API only | `set_account_webhook` + view field |
| Pending transfer finalize | No | `finalize_transfer` |
| Apps + SpacetimeAuth tickets | No | §7.9 reducers/views |
| Role-granular ACL UI | TODO in template | `Role` + grant/revoke |
| Admin credit/custom address | API | `create_account` rules |
| Ledger audit | API | `ledger_audit` view |
| Market | Commented menu only | Non-goal unless reopened |

---

## Reactivity map (Go → STDB edge)

| Go mechanism | Used on | Target on Topcoat |
|--------------|---------|-------------------|
| NATS + SSE full `#page-content` patch | Accounts list, Transfers list | STDB subscribe `my_accounts` / `my_transfers` → Datastar patches |
| Datastar form POST → PatchElements | Create account, account admin actions | Call reducers/procedures → re-query or subscribe-driven patch |
| Datastar POST → PatchSignals only | Transfer send, payment request success | Same pattern or toast + list update via sub |
| Datastar GET partial | Recipient fieldset | `account_directory` query or module/edge HTTP search |
| Last-Event-Id / Send-Initial-State | Transfers SSE reconnect | SSE reconnect + `ensure_bearer` + einro pool (refactor C5 / §5.3) |

Realtime product goal (refactor §10): replace NATS transfer subjects with STDB row/view updates; full fragment reload is fine initially.

---

## Suggested build order (parity while redesigning)

Aligned with refactor §8.8 / H\*, adjusted for “real app first”:

1. **App shell** + authed gate + username from `my_user`  
2. **Accounts list** + create + live balances (**H1**)  
3. **Transfers** list + send + recipient search + live updates (**H3**)  
4. **Account admin** primary / members / tokens (**H2**)  
5. **Payment request** (**H4**)  
6. **Logout** wired in chrome (**H5**)  
7. **App home** redesign (low Go surface)  
8. Stretch beyond Go UI: webhooks, apps, pending finalize, richer roles  

Prerequisites already largely landed on Topcoat: BitAuth, STDB connect-as-user, einro pool (C1–C3). Marketing home (D2) and BitAuth login (D3) are in progress / partial.

---

## Non-HTML surfaces

**Not** part of this page-port checklist. Still required for product cutover — own them in [spacetimedb-refactor.md](./spacetimedb-refactor.md):

| Surface | Where tracked |
|---------|----------------|
| Module HTTP + **edge reverse-proxy** of JSON API (new path shape, not legacy `/api`) | §8.5, §8.7 **I1–I3**, §9 |
| Account API tokens (validation in module; edge forwards `Authorization`) | §7.10, §9.3 |
| Direct STDB app clients (SpacetimeAuth + `account_member`) | §7.9, §9.2 |
| Admin HTTP that used edge `ADMIN_KEY` | Dropped; module `User.is_admin` only |

Go `/api/*` routes (reference only — do not preserve path shape):

- Public: ledgers list, account search, ping  
- Account-token: account get, transfers list/get/create, webhook get/put/delete, ping  
- Admin-key (Go only): create ledger, audit, create account, patch address/balance, get user  

---

## How to use with agents

1. Start from [spacetimedb-refactor.md](./spacetimedb-refactor.md) for architecture, module schema, and edge inventory.  
2. Use **this doc** for HTML app feature parity (content, actions, reactivity, build order).  
3. Tick surfaces here and §8.7 H* as they land; update both when product scope changes.  
4. Live module schema in `spacetimedb/src/` wins on domain drift; update this doc when UI capabilities change.
