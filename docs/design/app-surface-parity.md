# App surface parity (Go HTML → Topcoat)

**Status:** Working inventory for edge app port  
**Date:** 2026-08-08  
**Related:** [spacetimedb-refactor.md](./spacetimedb-refactor.md) (§8.7 H*, §8.8, §9–§10)  
**Source:** Live Go routes (`internal/routes/routes.go`), handlers (`internal/handlers/app.go`, `handlers.go`), templates (`web/templates/pages/*`, chrome components).

This document is the **hard feature-parity floor** for browser HTML surfaces. Redesign nav, headers, and layout freely; preserve the **capabilities** listed here unless this doc or the refactor doc is updated.

**Do not copy Go HTML.** H1/H2 were reimagined (asset + balance first, noun **Accounts**, marketing-like cards). Remaining pages (H3+) must follow that, not `web/templates/pages/*` layout or styling. Go templates are a capability reference only.

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
| 3 | Logout | Clear Stelo session → home | No | H5 **done** |
| 4 | App home | Something for logged-in user (Go: greeting only) | Optional | — |
| 5 | Accounts list | List wallets + balances; create debit (credit/custom addr if platform admin); open detail | **Yes** (balances) | H1 |
| 6 | Account home | Primary; members add/remove/roles; label; API tokens (Admin+) | **Yes** | H2 **(home + tokens done; webhook/apps later)** |
| 7a | Transfer send | From-account, recipient search, amount, memo, idempotency | **Yes** (bal) | H3a **done** |
| 7b | Activity | Live transfer history (all or one account) | **Yes** | H3b **done** |
| 8 | Payment request | Query-prefilled pay flow | No (one-shot submit) | H4 **done** |

**Chrome capability (Topcoat Option A, 2026-08-08):**  
Desktop/tablet top: logo→`/app` · Accounts · Activity · Transfer▾ (Send / Deposit / Withdraw) · username→`/app/me`.  
Mobile top: logo + username only; bottom: Accounts · Activity · Transfer (send).  
Logout lives on `/app/me` (profile page — more content planned). **`POST /logout`** clears Stelo cookies only (not BitAuth).

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

### 4. Logout — `POST /logout` (**done**)

**Design:** H5 · **Topcoat:** Stelo-only from `/app/me`

| | |
|--|--|
| **Content** | You page: username + Session card + Log out |
| **Actions** | `POST /logout` clears Stelo cookies → `/`. Does **not** call BitAuth `end_session`. No GET alias. |
| **Reactive** | None (normal form POST) |
| **Gaps** | Account age / wallet stats / more settings still later on `/app/me` |

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

**Design:** H1 · **Topcoat:** redesigned portfolio (2026-08-12) · **Go template:** `app-accounts.html.tmpl` (do not copy layout) · **Shell:** `accounts`

| | |
|--|--|
| **Content** | Heading **Accounts**. Debit accounts grouped by ledger (name + kind in plain language). Each card: large balance, optional **label** nickname, `#address`, Yours vs Shared·role·owned by, Primary pill; **Copy** for the address. Credit accounts in a trailing **Issuer accounts** section, still grouped by ledger, Credit chip. Empty: hold assets / receive from other players (no hardcoded asset name). |
| **Actions** | **New account** sheet: pick asset (cards), optional label, helper copy uses `@bitcraft_username`. First debit on a ledger is **auto-primary**. Platform admin **Advanced**: Credit kind + custom address. Whole card (except Copy) links to `/app/accounts/{id}` (H2). |
| **Reactive** | GET SSR from `my_accounts` + public `ledger`. `data-init="@get('/app/accounts/updates')"` — long-lived STDB subscribe on `my_accounts` → patch `#accounts-list` only (create sheet / signals survive). First SSE connect seeds `Last-Event-Id` with an empty patch (no list remorph). Reconnect (`Last-Event-Id` set) sends one snapshot. Subscribe-apply `on_insert`s are ignored. Multi-row txns coalesce to one fat morph via a watch slot. POST `create_account` is command-only (PatchSignals close form or `$createError`). |
| **Gaps** | No in-list Send (Send lives on H2 / Transfer). |

**Related routes (not separate pages):**

| Route | Role |
|-------|------|
| `GET /app/accounts/updates` | SSE stream for list patches |
| `POST /app/accounts` | Create debit account → PatchSignals only; list via `/updates` |

---

### 7. Account home — `GET /app/accounts/{account_id}`

**Design:** H2 · **Topcoat:** done (2026-08-12, home slice) · **Go template:** `app-account.html.tmpl` (do not copy layout) · **Shell:** accounts · **Authz:** any member (Read+)

| | |
|--|--|
| **Content** | Back to Accounts. Title = **label** or ledger name; large **balance**; `#address` + Copy; Primary / Credit / Shared·role chips. Debit: receive copy uses `@bitcraft_username`. **People**: users + apps, role, (you). **API tokens** (Admin+): label, relative time, minted-by; secret never listed. |
| **Actions** | Owner + debit: set/clear primary (`POST .../primary`). Write+ debit: **Send** → `/app/transfer`. Admin+: save label (`POST .../label`); search public `user` then grant Identity (`POST .../members`); change role; remove; **create token** (`POST .../tokens`, secret once in `$newToken`); **revoke token** (`POST .../tokens/revoke`). Non-owner: **Leave**. Owner can promote another member to Owner (clear primary first — module rule). |
| **Reactive** | SSR from `my_accounts` + `my_accounts_members` + `my_accounts_tokens` (+ public `user` for minter names). `data-init` → `GET .../updates` live sub patches `#account-home` only (create/reveal sheets + signals survive). Mutations are command-only PatchSignals (`$accountError`, `$tokenError` / `$newToken`, `$leftAccount` → `/app/accounts`). |
| **Gaps (H2 leftovers — do not rebuild people/primary/tokens)** | Request link builder (**Read+**, not Admin+). Recent transfers on this page. Deposit/Withdraw preselect. Webhook. App tickets. |

**Related routes:**

| Route | Role |
|-------|------|
| `GET /app/accounts/{id}/updates` | SSE: live `#account-home` |
| `POST /app/accounts/{id}/primary` | Owner; toggle primary |
| `POST /app/accounts/{id}/label` | Admin+; set/clear nickname |
| `GET /app/accounts/{id}/users` | Admin+ UI; debounce search → `#user-search-results` |
| `POST /app/accounts/{id}/members` | Admin+; grant/change role by Identity (`$memberId` / `$editMemberId`) |
| `POST /app/accounts/{id}/revoke` | Admin+; remove member by identity hex |
| `POST /app/accounts/{id}/leave` | Non-owner; revoke self |
| `POST /app/accounts/{id}/tokens` | Admin+; `create_account_token`; PatchSignals `$newToken` (secret once) |
| `POST /app/accounts/{id}/tokens/revoke` | Admin+; `revoke_account_tokens` |

---

### 8. Transfer send — `GET /app/transfer` **and** Activity — `GET /app/activity`

**Design:** H3 · **Topcoat:** send done (2026-08-12); Activity done (2026-08-13) · **Go template:** `app-transfers.html.tmpl` (capability reference — **do not copy layout**) · **Shell:** Transfer / Activity

Go combined send + history on one page. New chrome splits them. **Send + Activity done.**

#### 8a. Send — `GET /app/transfer` (**done**)

| | |
|--|--|
| **Content** | From-account picker (debit, Write+); recipient search; amount; optional memo; idempotency key; balance on selected account |
| **Actions** | Choose from-account; search/select recipient; submit `create_transfer`; start over after send. Account home **Send** preselects `?from={id}`. |
| **Reactive** | Recipient: debounce search of `account_directory` (edge filter/rank; `@`/`#` scope; same ledger; exclude sender; own other accounts show label) → pick → chip + clear. Submit → PatchSignals. |
| **Gaps** | No pending / finalize UI (module has it). History lives on `/app/activity` (H3b). |

**Related:**

| Route | Role |
|-------|------|
| `GET /app/transfer` | SSR send form; `?from=` preselect |
| `GET /app/transfer/recipients` | Debounced directory search → `#recipient-results` |
| `POST /app/transfer` | `create_transfer` (Write+ on sender); PatchSignals |

#### 8b. Activity — `GET /app/activity` (**done**)

| | |
|--|--|
| **Content** | Heading **Activity** + Send. Chips: All + one per account (label → `#address`). Day-grouped cards: counterparty, signed amount, verb, ledger, relative time, memo. Filter-relative verbs: Received / Sent / Moved (both legs yours + All) / Issued / Redeemed. Pending / Finalizing pills only (no finalize action). `?account=` deep-link. |
| **Actions** | Filter by account (client `data-show`; URL via `replaceState`). Send → `/app/transfer`. |
| **Reactive** | SSR from `my_transfers` + `my_accounts`. `data-init="@get('/app/activity/updates')"`. First SSE seed-only; reconnect one snapshot. Subscribe-apply ignored. Edge dedupes view doubles. Filter does not reopen the stream. |
| **Gaps** | No transfer detail page. No pending / finalize UI. No pagination (full view; follow-up **Q15** in the refactor doc). |

**Related:**

| Route | Role |
|-------|------|
| `GET /app/activity` | SSR chips + list; `?account=` |
| `GET /app/activity/updates` | SSE: live `#activity-body` |

---

### 9. Payment request — `GET /app/request?...` (**done**)

**Design:** H4 · **Topcoat:** Pay confirm (2026-08-14) · **Go template:** `app-request.html.tmpl` (capability reference — **do not copy**) · **Shell:** none (not a nav dest)  
**Query:** `ledgerid`, `recipientid`, `amount` required; `memo` optional (see `docs/payment-requests.md`)

| | |
|--|--|
| **Content** | Heading **Pay**. Invoice card: large qty + ledger, `to @user · #addr`, memo. From-picker (H3a cards; debit **Write+** on that ledger; recipient excluded; primary first). Available balance on the card. |
| **Actions** | Change from-account → **Pay {qty} {ledger}**. Insufficient → hint + disabled. Success → View activity / Accounts. |
| **Reactive** | Submit `@post('/app/request')` → PatchSignals `$sent` / `$sendError`. No SSE. Idempotency key on SSR. |
| **Gaps** | No live balance subscribe. Request-link **builder** is an H2 leftover (Read+). Not in nav. |

**Related:**

| Route | Role |
|-------|------|
| `GET /app/request` | SSR invoice + from-picker; bad/missing query → friendly invalid page (not 400) |
| `POST /app/request` | `create_transfer` (Write+ on sender); PatchSignals |

Go `POST /app/request/{id}/transfers` was Admin+. Topcoat matches the module / H3a (**Write+**).

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
| Account label set/clear | H2 Admin+ | `set_account_label` + `my_accounts.label` |
| Pending transfer finalize | No | `finalize_transfer` |
| Apps + SpacetimeAuth tickets | No | §7.9 reducers/views |
| Role-granular ACL UI | H2 people section | `Role` + grant/revoke |
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
2. ~~**Accounts list** + create + live balances (**H1**)~~ **done**  
3. ~~**Account home** primary / people / label (**H2**)~~ **done** (leftovers: webhook, apps, request builder Read+, recent)  
4. ~~**Transfer send** (`/app/transfer`) — from-account, recipient search, `create_transfer` (**H3a**)~~ **done**  
5. ~~**Activity** (`/app/activity`) — live `my_transfers` (**H3b**)~~ **done**  
6. ~~**Payment request** (**H4**)~~ **done**  
7. ~~**Logout** wired in chrome (**H5**)~~ **done** (`POST /logout` from `/app/me`; Stelo-only)  
8. **App home** redesign (low Go surface)  
9. Stretch: H2 leftovers (webhook, apps), pending finalize  

Prerequisites already largely landed on Topcoat: BitAuth, STDB connect-as-user, einro pool (C1–C3). Marketing home (D2) and BitAuth login (D3) are in progress / partial.

---

## Non-HTML surfaces

**Not** part of this page-port checklist. Still required for product cutover — own them in [spacetimedb-refactor.md](./spacetimedb-refactor.md):

| Surface | Where tracked |
|---------|----------------|
| Module HTTP + **edge reverse-proxy** of JSON API (`/api` + module remainder; not Go `/api/accounts/{id}`) | §8.5, §8.7 **I1–I3**, §9 |
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
