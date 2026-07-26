# Design Doc: SpacetimeDB Refactor

**Status:** Outline + **P0 domain reducers in progress** (auth skeleton done; ledger/account create landed — see §22)  
**Date:** 2026-07-24 (updated 2026-07-25)  
**Author:** Stelo maintainers + design discussion  
**Related:** Current stack is Go + SQLite (sqlc/goose) + embedded NATS/JetStream + Datastar; STDB module + BitAuth OIDC path land in parallel

---

## 1. Summary

Replace SQLite and NATS with **SpacetimeDB** as the single system of record, application logic host, realtime sync layer, and (eventually) direct third-party client surface.

Split the product into two deployables:

| Piece | Role |
|-------|------|
| **SpacetimeDB module** (Rust) | Tables, reducers, views, procedures. All domain/financial logic. |
| **Lite webserver** (Go) | HTTP/HTML/JSON bridge. BitCraft OIDC → browser cookie holding STDB token. Datastar patches. Thin REST façade over reducers/views. **No domain rules.** |

Production module runs on **SpacetimeDB mainnet/maincloud**. Local dev runs a local SpacetimeDB instance. Existing production balances are small/tester-only; a **manual data import** cutover is acceptable.

---

## 2. Goals

1. **Single source of truth** for accounts, balances, transfers, permissions, tokens, webhooks.
2. **All money-moving and authz rules live in the module** (reducers + private tables + views).
3. **Realtime UI** without NATS: STDB subscriptions → edge re-renders Datastar HTML fragments.
4. **Third-party integration path:**
   - Near term: existing-style JSON HTTP API as a **façade** over STDB.
   - Later: partners (and users) connect with native STDB clients under multi-tenant views.
5. **Browser acts as an authenticated STDB identity**, with the lite server proxying as that user (token-in-cookie), not as a privileged superuser.
6. Remove operational dependency on embedded JetStream (sessions KV, transfer pub/sub, webhook work queue).

## 3. Non-goals (this refactor)

- Rewriting the HTML stack away from Go `tmpl` + Datastar.
- Switching the lite edge off Go (e.g. to Rust/TS) solely for typed query builders.
- Perfect zero-downtime dual-write migration (testers can be re-imported).
- Full threat-model / abuse / rate-limit design (tracked as follow-up).
- Per-user SpacetimeDB databases (we use **one multi-tenant module**).

---

## 4. Current architecture (baseline)

```text
Browser ──HTTP/SSE──► Go (chi + Datastar + tmpl)
                        │
            ┌───────────┼───────────┐
            ▼           ▼           ▼
         SQLite      NATS/JS      BitJita /
         (sqlc)   (KV, pub/sub,   BitCraft
                   webhooks)      login
```

| Concern | Today |
|---------|--------|
| Persistence | SQLite file (`user`, `ledger`, `account`, `account_permission`, `transfer`, `transfer_idempotency`) |
| Domain logic | `internal/accounts/*` in process with the HTTP server |
| Realtime | NATS subjects e.g. `accounts.transfers.{sender}.{receiver}` → SSE HTML patches |
| Sessions / API tokens | JetStream KV |
| Webhooks | JetStream work queue + HTTP worker |
| Auth | BitCraft login handshake (BitJita-style), cookie `sid`, account tokens in KV |

Key domain invariant (must preserve):

> For each ledger,  
> **Σ balances of debit-normal accounts − Σ balances of credit-normal accounts = 0**  
> (double-entry conservation).

---

## 5. Target architecture

```text
Browser
  │  cookie: STDB access token (user identity)
  │  (and optional sid/session metadata if still needed)
  ▼
Lite webserver (Go)  ── acts as that user ──►  SpacetimeDB (mainnet / local)
  │   - HTTP routes, cookies, OIDC callback           │
  │   - Datastar HTML from tmpl                       │  Rust module:
  │   - JSON façade → CallReducer / query             │  private tables
  │   - STDB client (digitalxero)                     │  reducers, views
  │   - NO balance/transfer/authz rules               │  webhook procedures
  ▼
Static assets (CSS/JS)
```

### 5.1 Responsibility split

| Layer | Owns | Does not own |
|-------|------|--------------|
| **Module (Rust)** | Schema, mutations, authz, invariants, outbox/webhooks, public read surface via views | HTML, cookies, OIDC browser redirect UX, REST shape of legacy API |
| **Lite edge (Go)** | OIDC login UX, cookie storage of STDB token, Datastar rendering, JSON façade, STDB client I/O | Whether a transfer is valid; what rows a user may see |

**Rule of thumb:** if the answer affects money or privacy, it belongs in the module.

### 5.2 “Edge as the user” (session model — preferred)

**Decision:** Prefer **BitAuth OIDC** (`https://auth.trinit.is/`, BitCraft sign-in) over BitJita-style login. Store the OIDC **ID token** in an HTTP-only cookie. The lite webserver uses that cookie on each request to open (or later pool) an STDB connection **as that identity** via `WithToken`.

Desired properties:

- Edge is **not** a god-mode service identity for user reads/writes.
- Module authorization is based on `ctx.sender()` (STDB `Identity`), so later direct clients reuse the same reducers/views.
- Page render path: **one STDB session / identity context** loads the data needed for the template (via one-off queries or short-lived subscribe), rather than many ad-hoc SQLite round-trips scattered through handlers. Goal: avoid “N independent DB calls per render” as a pattern; batch via views where possible.

**Implemented (spike — parallel routes, legacy login still live):**

| Item | Choice |
|------|--------|
| IdP | BitAuth — `https://auth.trinit.is/` (OIDC Auth Code + PKCE, confidential client) |
| Edge OIDC library | `github.com/coreos/go-oidc/v3` + `golang.org/x/oauth2` |
| Cookie (ID token for STDB) | `stdb_id_token` (HttpOnly, SameSite=Lax; Max-Age from JWT `exp`) |
| Cookie (refresh) | `stdb_refresh_token` when `offline_access` granted (~14d BitAuth) |
| Cookie Secure flag | `BITAUTH_SECURE_COOKIES` / prod; local HTTP often `false` |
| Login routes | `GET /auth/bitauth/login`, `/callback`, `/logout`, `/session` |
| STDB smoke | `GET /auth/bitauth/stdb-connect` — digitalxero client, no codegen |
| STDB client | `go.digitalxero.dev/spacetimedb-client` |
| Do not | Overwrite `stdb_id_token` with short-lived websocket tokens returned on connect |

**Still open / later:**

- Account API tokens for `/api/accounts/{id}/*` remain a separate mechanism (capability tokens), not the browser cookie.
- Legacy `/login` + `sid` JetStream session path remains until cutover.
- Connection pooling by Identity (v1 = per-request connect).

### 5.3 Connection model

| Phase | Behavior |
|-------|----------|
| **v1** | New STDB connection (or connect+query+disconnect) **per HTTP request** is acceptable for API and page loads. SSE/Datastar update streams may hold a longer-lived connection for the duration of the SSE. |
| **Later** | Pool connections **by identity** for hot users (optional optimization). |

JSON API: **one-off queries / reducer calls** first; identity-based pooling later (same as above).

---

## 6. Decisions log

| # | Decision | Choice | Notes |
|---|----------|--------|--------|
| D1 | Module language | **Rust** | Error handling + type system for financial logic |
| D2 | Lite edge language | **Go** | Keep chi, tmpl, Datastar; digitalxero client |
| D3 | Data access control | **Private tables + public views + sender-checked reducers** | Design as if clients connect directly from day one |
| D4 | Webhooks | **Outbox table + scheduled procedure (HTTP from module)** | Replaces JetStream work queue |
| D5 | Browser session | **BitCraft OIDC → STDB token in cookie; edge calls STDB as user** | Avoid privileged edge for user data; minimize multi-hop app logic |
| D6 | Third-party API | **HTTP JSON façade over reducers/views** | Preserve existing docs shape where practical |
| D7 | Multi-tenancy | **Single module, multi-tenant views** | View output depends on caller identity |
| D8 | Hosting | **Mainnet/maincloud prod; local STDB for dev** | |
| D9 | Migration | **Manual/import cutover OK** | Tester-scale production data |
| D10 | Go queries | **SQL/view name strings + community codegen for types** | No official Go query builder; keep edge queries simple |
| D11 | Typed query builder | **Not a reason to rewrite edge** | Module + views carry correctness |
| D12 | Module path | **`spacetimedb/`** (CLI default) | Not `module/`; `spacetime.json` `module-path` |
| D13 | Local STDB config | **`spacetime.json` + `spacetime.dev.json`** | Dev: `server: local`, DB name `stelofinance`; data dir `tmp/spacetimedb` |
| D14 | Browser IdP | **BitAuth** (`auth.trinit.is`) | Auth Code + PKCE; confidential client secret on Go only |
| D15 | STDB principal | **`Identity` = f(iss, sub)** as `User` PK | BitAuth `sub` is stable numeric player id; username is `preferred_username` |
| D16 | User bootstrap | **`client_connected` only** (no separate `ensure_user`) | Upsert on connect; no JWT → reject (except owner) |
| D17 | App admin | **`User.is_admin: bool`** + `require_admin` | Bootstrap first admin via owner SQL; not SpacetimeAuth/JWT roles for now |
| D18 | DB owner / CLI | **Store owner in `config` at `init`** | Owner may connect for SQL without BitAuth; not product admin |
| D19 | Private tables | **Default private; public only `ledger` (catalog)** | Host enforces client visibility; owner SQL can read private |
| D20 | Composite uniqueness | **Reducer-enforced** (STDB 2.7 has no multi-col unique) | Indexes for lookup (e.g. idempotency `by_account_and_key`) |
| D21 | Idempotency storage | **Separate `transfer_idempotency` table** | Scope `(account_id, key)` → transfer + request_hash |
| D22 | Go STDB client | **digitalxero** `go.digitalxero.dev/spacetimedb-client` | Codegen not required for connect-only smoke |
| D23 | Connection pooling | **Deferred** | v1 per-request connect with cookie token |
| D24 | Account create authz | **Debit open; Credit admin-only; custom address admin-only** | Maps former GA vs SRA/PRA; owner is always `ctx.sender()` |
| D25 | Primary account `user_id` | **`Identity` + `ZERO` sentinel** (not `Option`) | Enables `user_id` index filter for “one primary per user per ledger” |

---

## 7. Module design (Rust)

**Code location:** `spacetimedb/` (`tables.rs`, `lib.rs`). Source of truth for live schema is the Rust module; this section summarizes decisions.

### 7.1 Table inventory (current spike schema)

All core tables **private** unless noted. Enums used instead of opaque integer codes where practical.

| Table | Accessor | Purpose | Notes |
|-------|----------|---------|--------|
| `config` | `config` | Singleton owner Identity | Written in `init` from `ctx.sender()` (publisher). PK = `owner` |
| `user` | `user` | Stelo user profile | **PK = `Identity`**. Unique `bitcraft_username`. `is_admin` (default false) |
| `ledger` | `ledger` | Asset type / scale / kind | **Public** catalog. `LedgerKind`: Digital / Derivation / Physical |
| `account` | `account` | Wallet / balances | `AccountKind` Credit/Debit; `user_id` = primary or **`Identity::ZERO`**; multi-col index `by_user_and_ledger`; single-col `ledger_id` + `address` |
| `account_user` | `account_user` (`AccountUser`) | User ↔ account ACL | `UserRole`; multi-col `by_account_and_user`; single-col `user_id` |
| `transfer` | `transfer` | Transfer records | `TransferKind` + `TransferState`; optional pending/posted amounts; `finalized_at` optional |
| `transfer_idempotency` | `transfer_idempotency` | Idempotency map | Auto-inc PK; index `by_account_and_key` on `(account_id, key)` |
| `account_token` | — | API tokens (not yet) | Hashed only; later |
| `webhook_outbox` | — | Webhook jobs (not yet) | Later |

**Public surface:** prefer **views** for balances/PII; public base tables only when intentionally world-readable (`ledger`).

**Private table visibility (platform, not app code):**

- Normal clients (including BitAuth) **cannot** query private tables directly.
- Reducers/views on the server **can** read/write private tables.
- **Database owner** (publisher Identity) may read private tables via `spacetime sql` for debugging.
- `spacetime sql --anonymous` respects client visibility (use to simulate unprivileged clients).

**STDB constraints:** no multi-column unique / composite PK in 2.7; no foreign keys. Uniqueness of composites (address+ledger, account+key, etc.) is **enforced in reducers** when those paths land. Btree indexes support lookups.

### 7.2 Identity, connect policy & admin

```text
BitAuth OIDC (browser)
    → Go /auth/bitauth/* (PKCE + client secret)
    → cookie stdb_id_token = OIDC ID token
    → Go connects STDB WithToken(id_token)
    → host verifies JWT, Identity = f(iss, sub)
    → client_connected:
         - if sender == config.owner → allow (ops CLI), no User row
         - else require BitAuth iss + aud
         - upsert User { id: sender, bitcraft_username from preferred_username, is_admin: false }
```

**BitAuth claims (observed in dev):**

| Claim | Use |
|-------|-----|
| `iss` | Must be `https://auth.trinit.is/` |
| `aud` / `azp` | App client id (e.g. `nintron-stelofinance`) — must match module constant / `BITAUTH_CLIENT_ID` |
| `sub` | **Stable** BitCraft player id (numeric string) — basis of STDB `Identity` |
| `preferred_username` | Display name stored on `User` (required; connect fails if missing) |
| `name` | Display only (not used for principal) |

Note: BitAuth marketing text may say `sub` is username; **live tokens use stable numeric `sub`**. Trust observed tokens; do not use username as principal.

**Principals:**

| Role | Mechanism |
|------|-----------|
| **Player** | BitAuth JWT → `Identity` → `User` row |
| **App admin** | `User.is_admin == true`; `require_admin(ctx)` on privileged reducers |
| **DB owner** | `config.owner` from `init`; CLI SQL / connect for ops; **not** automatic product admin |
| **Module identity** | `ctx.database_identity()` — scheduling / “is this the module?”, not the publisher |

**Admin bootstrap (chicken-and-egg):**

1. Publish module (`init` stores owner).
2. Player logs in via BitAuth → `User` created with `is_admin = false`.
3. Owner runs SQL once:  
   `UPDATE user SET is_admin = true WHERE bitcraft_username = '…';`
4. Later: admin-only reducers may `grant_admin` / `revoke_admin`.

STDB docs’ JWT `roles` claim pattern is preferred if BitAuth ever issues roles; until then **DB flag on `User`**.

Reducers must treat `ctx.sender()` as the principal. Map:

`Identity` → `user` → `account_user` / ownership on `account`.

Edge `ADMIN_KEY` remains temporary break-glass for legacy HTTP only — not module authz long-term.
### 7.3 Views (multi-tenant, caller-dependent)

Views use `ViewContext` and filter by `ctx.sender()`. Prefer indexed lookups; use query-builder views where joins/filters are declarative.

| View (proposal) | Returns | Auth idea |
|-----------------|---------|-----------|
| `my_user` | Current user profile | Caller’s row only |
| `my_accounts` | Accounts caller can see | Via `account_user` (and primary ownership) |
| `my_account` / detail fields | Single account + balances | Must have `PermReadBal` or admin |
| `my_transfers` | Transfers involving caller’s accounts | Permission-gated |
| `ledger_list` | Public ledger catalog | Anonymous or authenticated; no secrets |
| `account_lookup` | Minimal recipient search (id, address, username) | Enough for transfer UI; no full balances of strangers |
| `account_webhook_config` | Webhook URL for accounts caller admins | Admin only |

**Invariant support:** admin-only `ledger_audit` view or reducer that checks  
`sum(debit_normal balances) - sum(credit_normal balances) == 0` per ledger (port of current audit endpoint).

### 7.4 Reducers (mutations)

All reducers: validate sender, load permission, enforce domain rules, mutate only via this path.

| Reducer (proposal) | Responsibility |
|--------------------|----------------|
| `client_connected` (**done**) | Owner bypass; BitAuth iss/aud; upsert `User` (no separate `ensure_user`) |
| `require_admin` (**helper done**) | Gate admin reducers on `User.is_admin` |
| `create_account` (**done**) | Owner = `ctx.sender()`. **Debit** open; **Credit** admin; **custom `address: Some`** admin; `None` auto-generates |
| `update_account_address` | Admin/system |
| `set_account_user` | Link/unlink primary user |
| `grant_permission` / `revoke_permission` | Account ACL |
| `create_transfer` (**done**) | Auth by kind (asset/liability posted-only; redeem/issue posted vs pending rules); idempotency `(sender, key)` |
| `finalize_transfer` (**done**) | One-shot on `Pending` only; post→`PostPending` (refund rest); void→`VoidPending`; state-based idempotent replay |
| `set_webhook` / `clear_webhook` | Account webhook URL |
| `create_account_token` / `revoke_account_token` | API tokens (return raw token **once** to caller) |
| `create_ledger` (**done**) | **`require_admin`**; public catalog row |
| `grant_admin` / `revoke_admin` | Admin-only (after first owner-SQL bootstrap) |
| `admin_patch_balance` | Admin only; auditable; must preserve or deliberately break invariant with care |
| `admin_import_*` | One-shot migration helpers (optional; can be CLI + publish instead) |

**`create_transfer` must preserve:**

- Amount ≥ 1
- Distinct sender/receiver accounts
- Same ledger
- Compatible account codes → transfer code (liability / asset / issue / redeem)
- Balance sufficiency (posted path)
- Memo length limits
- Idempotency key required; same key + same hash → replay; same key + different hash → conflict
- Atomic balance updates + transfer insert + idempotency insert
- Outbox rows for sender/receiver webhooks if configured

### 7.5 Procedures (side effects)

| Procedure | Role |
|-----------|------|
| `deliver_webhook` (scheduled) | Read outbox job, `http` POST, update status / reschedule / dead-letter |

Constraints:

- Do not hold a DB transaction open across HTTP.
- Pattern: `with_tx` read job → HTTP → `with_tx` write result.
- Preserve operational safeguards from today’s worker where possible: timeouts, no redirect following, max attempts, backoff, User-Agent, body schema compatible with existing webhook docs.

### 7.6 Accounting invariant

**Hard invariant (per ledger):**

```text
Σ balance(debit-normal accounts) − Σ balance(credit-normal accounts) = 0
```

Balance definition should match current app semantics (posted ± pending as defined today).

Enforcement options (pick during implementation):

1. **Structural:** only `create_transfer` (and tightly controlled admin) mutates balances; unit/integration tests + audit reducer.
2. **Audit:** scheduled or on-demand `audit_ledger` that fails/alerts if non-zero.
3. Avoid unrestricted `admin_patch_balance` in production without paired offsetting entry.

---

## 8. Lite webserver design (Go)

### 8.1 Dependencies

- Keep: chi, tmpl, Datastar, static assets, Fly deployment for the edge.
- Add: `go.digitalxero.dev/spacetimedb-client` (or successor).
- Remove eventually: modernc sqlite, goose runtime path, embedded NATS/JetStream, sqlc-generated DB access for domain.

### 8.2 Auth & cookies

**Browser flow (BitAuth — implemented on parallel routes):**

1. `GET /auth/bitauth/login` → BitAuth authorize (PKCE S256, scopes `openid profile` [+ `offline_access`]).
2. `GET /auth/bitauth/callback` → code exchange with client secret; verify ID token (iss/aud/nonce).
3. Set cookies: `stdb_id_token` (raw ID token), optional `stdb_refresh_token`.
4. On demand / later every page: `WithToken(stdb_id_token)` → STDB connect → `client_connected` upserts `User`.
5. `GET /auth/bitauth/stdb-connect` — smoke JSON with identity (digitalxero; no table codegen).
6. `GET /auth/bitauth/session` — JSON claims from cookie (no raw token returned).
7. `GET /auth/bitauth/logout` — clear cookies; optional BitAuth end_session.

**Env (placeholders in `.env`):**  
`BITAUTH_ISSUER`, `BITAUTH_CLIENT_ID`, `BITAUTH_CLIENT_SECRET`, `BITAUTH_REDIRECT_URL`, `BITAUTH_LOGOUT_REDIRECT_URL`, `BITAUTH_OFFLINE_ACCESS`, `BITAUTH_SECURE_COOKIES`, `STDB_HOST`, `STDB_DATABASE`.

**Packages:** `internal/bitauth`, `internal/stdb`, handlers under `internal/handlers/bitauth.go`.

**Account token API (third party):**

- Header `Authorization: <token>` as today (legacy).
- Preferred end state: module stores only hashes; edge never reconstructs god rights from KV.

### 8.3 Request handling patterns

**Page load (HTML):**

```text
cookie token → connect/query as user
  → OneOffQuery / subscribe-applied snapshot of needed views
  → fill tmpl structs
  → respond HTML
```

Prefer **few view queries** that return render-ready shapes over many point lookups.

**Datastar live updates:**

```text
SSE open → STDB Subscribe to my_accounts / my_transfers (etc.)
  → on insert/update/delete callbacks → re-render fragment → PatchElements
  → on disconnect → unsubscribe / close STDB conn
```

**JSON façade:**

```text
HTTP → validate transport auth (cookie or account token)
     → CallReducer / OneOffQuery
     → map STDB errors to HTTP status JSON
```

No re-implementation of transfer math in Go.

### 8.4 Types & codegen

Official `spacetime generate` does **not** support Go.

Plan:

1. Module schema is source of truth (Rust).
2. Generate Go bindings via digitalxero / community tooling where available (`stdb-go` / schema fetch generate).
3. Fallback: checked-in generated structs + BSATN codecs; regenerate in CI when module changes.
4. Edge subscriptions use **simple SQL strings against views** (`SELECT * FROM my_accounts`), not ad-hoc joins on private tables (private tables are invisible to clients anyway).

### 8.5 What gets deleted from Go

- `database/queries/*`, `database/gensql/*`, goose migrations for app schema (replace with module publish).
- `internal/accounts/*` domain logic (ported to Rust).
- NATS publish of transfer events; JetStream sessions/webhooks streams.
- Embedded NATS server bootstrap in `web/web.go`.

What remains thin:

- `internal/handlers/*` as protocol adapters
- `web/templates/*`
- middleware for cookie/token extraction only (not ACL math)

---

## 9. API compatibility

### 9.1 External JSON API

Keep documented routes under `/api` as a **façade**:

| Area | Strategy |
|------|----------|
| Ledgers list/create/audit | Map to views + admin reducers |
| Accounts search / get | Views |
| Transfers list/get/create | Views + `create_transfer` |
| Webhook get/put/delete | Views + reducers |
| Ping | Local or cheap reducer/view |

Breaking changes: minimize; if STDB IDs differ from old SQLite integers, document migration (u64 vs int64) and update docs.

### 9.2 Direct STDB clients (phase 2)

Same module, same views/reducers. Partners use official SDKs (TS/Rust/C#) with user or service OIDC. Edge is optional for them.

---

## 10. Realtime: NATS → STDB

| Today | Target |
|-------|--------|
| Publish `EventTransfer` on NATS after commit | Transfer row commit updates subscriptions automatically |
| Edge subscribes to subjects per account id | Edge subscribes to **views** scoped by identity |
| Full page fragment reload on any event | Same UX initially (simple); later finer-grained patches optional |

Webhook delivery is **not** realtime fanout; it is outbox/procedure.

---

## 11. Webhooks

### 11.1 Flow

```text
create_transfer reducer (single tx)
  → insert transfer + update balances + idempotency
  → if account.webhook set: insert webhook_outbox rows (sender and/or receiver)
  → schedule deliver_webhook

deliver_webhook procedure
  → load job
  → HTTP POST JSON EventTransfer-compatible body
  → success: mark delivered
  → failure: backoff / retry / dead-letter after max attempts
```

### 11.2 Payload compatibility

Preserve fields from current `EventTransfer` JSON where possible so existing integrators keep working:

`id`, `debitAccId`, `creditAccId`, `amount`, `ledgerId`, `code`, `memo`, `createdAt`.

---

## 12. Migration plan

### 12.1 Principles

- Tester-scale production: **export → transform → import** is OK.
- Prefer freeze window over dual-write complexity.
- Validate ledger invariant after import.

### 12.2 Phases

| Phase | Work | Exit criteria |
|-------|------|----------------|
| **P0 — Spike** | Module skeleton + BitAuth connect + tables; then create_transfer, views, one page/Datastar | End-to-end transfer visible in UI via STDB |
| **P1 — Auth** | BitAuth OIDC + cookie + `client_connected` (largely done in P0 parallel path); cut over `/app` off JetStream `sid` | Login works without BitJita/JetStream login KV |
| **P2 — Domain complete** | Permissions, idempotency, ledgers, tokens, audit | Parity with current `internal/accounts` behavior |
| **P3 — Webhooks** | Outbox + scheduled procedure | Delivery + retries without NATS |
| **P4 — API façade** | Port `/api` routes to STDB | Docs still valid; integration tests pass |
| **P5 — Import** | Script/reducer import of users/accounts/balances/transfers (or rebuild balances from transfers) | Audit invariant holds; tester accounts usable |
| **P6 — Cutover** | Deploy module to mainnet; point edge at mainnet; decommission SQLite+NATS volumes | Stable prod; old DB read-only archive |
| **P7 — Cleanup** | Remove dead Go deps and NATS embed | Smaller binary/ops surface |

### 12.3 Data import sketch

1. Export SQLite tables to JSON/CSV.
2. Map old integer IDs → new module IDs (keep a mapping table during import).
3. Insert users, ledgers, accounts (balances either imported or recomputed from transfers).
4. Insert permissions, transfers, idempotency keys if still relevant.
5. Run `audit_ledger` for every ledger.
6. Notify testers; optional re-login via OIDC (sessions not migrated).

---

## 13. Open questions / follow-ups

| ID | Topic | Status | Notes |
|----|-------|--------|-------|
| Q1 | Full threat model (abuse, spam transfers, energy, anonymous connect policy) | **Follow up later** | Issuer/aud gate exists for BitAuth; expand rate limits etc. |
| Q2 | Cookie details: name, Max-Age, rotation, logout | **Mostly decided** | `stdb_id_token` / `stdb_refresh_token`; refresh rotation & revoke TBD |
| Q3 | OIDC claim mapping | **Decided (dev)** | Stable `sub` = player id; `preferred_username` = display; see §7.2 |
| Q4 | Account API token validation path (edge hash lookup vs pure reducer) | Open | Prefer module-side verification |
| Q5 | Exact table/view/reducer names | **In flux** | Live schema in `spacetimedb/src/tables.rs` |
| Q6 | Pending transfer flags / states | Open | Schema has `TransferState` + optional amounts; logic incomplete |
| Q7 | digitalxero vs STDB protocol drift process | Monitor | Pinned `v0.6.0` for smoke; CI later |
| Q8 | Admin auth | **Decided for spike** | `User.is_admin` + owner SQL bootstrap; JWT roles if BitAuth adds them later |
| Q9 | Backups / PITR / disaster recovery on mainnet | Open | Ops runbook |
| Q10 | Connection pooling by identity | Deferred | v1 per-request OK; pool by **user Identity**, not OIDC client_id |
| Q11 | Whether import recomputes balances from transfers vs copies balances | Open | Recompute is safer if history complete |

---

## 14. Testing strategy

| Layer | What |
|-------|------|
| Module unit/integration | Reducer tests: valid transfer, insufficient funds, idempotent replay, idempotent conflict, cross-ledger reject, permission deny |
| Invariant tests | Random transfer sequences → ledger sums to 0 |
| View tests | User A cannot read user B balances via views |
| Edge smoke | OIDC dev path (or fixture token), HTML render, Datastar SSE update on transfer |
| Webhook tests | Outbox retry; HTTP mock; no open redirects |
| Import dry-run | Snapshot of prod export against local STDB |

---

## 15. Risks and mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Wrong authz → fund theft / data leak | Critical | Private tables; view filters; reducer checks; adversarial tests |
| Community Go client lag | High | Pin versions; thin edge; ability to shell out critical paths; monitor STDB releases |
| Token-in-cookie theft (XSS) | High | HttpOnly Secure cookies; tight CSP; no token in JS |
| Webhook SSRF from module HTTP | High | URL allowlist/block private ranges; no redirects; timeouts |
| Mainnet ops unfamiliarity | Medium | Local parity; runbooks; staged publish |
| Naming / API ID changes break clients | Medium | Façade stability layer; publish migration notes |
| Per-request connect latency | Medium | Pool later by identity; keep page queries few |

---

## 16. Success metrics

1. SQLite and NATS **not required** in production edge.
2. All transfers and balance changes happen only in module reducers.
3. Browser session uses STDB user identity (cookie token); views enforce isolation.
4. Existing JSON API consumers work via façade (or documented breaks only).
5. Webhooks deliver with durable retries without JetStream.
6. Ledger audit invariant holds post-import and under test suites.
7. A second client (e.g. small TS script) can call `create_transfer` / subscribe to `my_transfers` with a user token and see the same authz behavior as the website.

---

## 17. Suggested repo layout (post-refactor)

```text
stelofinance/
  spacetimedb/            # Rust SpacetimeDB module (CLI default path)
    src/lib.rs            # init, client_connected, require_admin
    src/tables.rs         # schema
    Cargo.toml
  spacetime.json          # database + module-path
  spacetime.dev.json      # server: local (committed shared dev)
  # spacetime.local.json  # personal overrides — gitignored
  cmd/app/
  internal/bitauth/       # OIDC client (go-oidc)
  internal/stdb/          # ConnectOnce helper (digitalxero)
  internal/handlers/      # bitauth routes + legacy
  Taskfile.yml            # stdb:start | publish | live | logs | reset
  tmp/spacetimedb/        # local STDB data-dir (gitignored via tmp/)
  docs/design/spacetimedb-refactor.md
```

**Local module workflow:** `task stdb:start` (one terminal) + `task stdb:live` (watch rebuild/publish). Wipe inconsistent local state with `rm -rf tmp/spacetimedb` if snapshot/identity errors appear.

---

## 18. Spike checklist (P0)

Use this to validate the design before full port:

- [x] Module crate + `spacetime.json` / `.dev.json` + Taskfile `stdb:*`
- [x] Private domain tables (+ public `ledger`); enums; idempotency table + index
- [x] `config.owner` at init; owner connect for CLI SQL
- [x] `client_connected`: BitAuth iss/aud + User upsert (`Identity` PK, `preferred_username`)
- [x] `User.is_admin` + `require_admin` helper (admin reducers TBD)
- [x] BitAuth OIDC parallel routes + cookies (`stdb_id_token`)
- [x] Go STDB connect smoke (`/auth/bitauth/stdb-connect`) — no codegen
- [x] Document OIDC claims + cookie names (this section / §5.2 / §7.2)
- [x] `create_ledger` (admin) + `create_account` (debit any user; credit admin)
- [x] `create_transfer` + idempotency + pending; `finalize_transfer` (void / post)
- [ ] Seed / bootstrap script (ledgers + issuer + sample wallets via reducers)
- [ ] `my_user` / `my_accounts` / `my_transfers` views filtered by sender
- [ ] Go edge: one-off query views / reducer call beyond smoke
- [ ] One app page rendered from STDB (parallel to legacy `/app`)
- [ ] SSE + subscribe → Datastar patch on transfer
- [ ] Prove a second identity cannot read the first identity’s view data

---

## 19. Appendix A — Current → target mapping

| Current component | Target |
|-------------------|--------|
| `gensql` models | Rust tables + generated Go types |
| `accounts.CreateTransfer` | `create_transfer` reducer |
| `accounts.EventTransfer` + NATS publish | STDB row updates + optional outbox |
| JetStream sessions KV | STDB token cookie (+ module user row) |
| JetStream account tokens | `account_token` table |
| JetStream webhook stream | `webhook_outbox` + procedure |
| `AppAccountsUpdates` NATS subs | STDB subscribe on views |
| `ADMIN_KEY` middleware | Temporary edge break-glass → `User.is_admin` + admin reducers |
| Goose migrations | `spacetime publish` module versioning |
| sqlc | Module query builder / views |
| BitJita-style login KV | BitAuth OIDC + `stdb_id_token` cookie |

## 20. Appendix B — Invariant reference

Double-entry (Stelo):

- Accounts have a **code** classifying debit-normal vs credit-normal (and issue/redeem paths).
- Transfers move value by increasing debits on one account and credits on another per transfer code rules (existing `TrLiability` / `TrAsset` / `TrIssue` / `TrRedeem`).
- Conservation: for each `ledger_id`, aggregate signed balances across accounts is zero.

Any admin balance patch must either:

- be expressed as an issue/redeem-style transfer, or  
- be paired with an offsetting adjustment and audit log.

---

## 21. Next actions

1. ~~Create `spacetimedb/` + local publish/dev loop~~ **done**.
2. ~~BitAuth OIDC + cookie + `client_connected` + owner/admin model~~ **done** (parallel path; legacy login remains).
3. ~~`create_ledger` + `create_account` (credit admin / debit open)~~ **done**.
4. **Next:** seed script; then multi-tenant views; wire one Go page off STDB; Datastar subscribe.
5. Cut over `/app` session from JetStream `sid` to BitAuth/STDB when ready (P1).
6. After fuller P0, expand this doc into an **implementation spec** (exact reducer signatures, error codes, cookie RFC polish).

---

## 22. Progress log (auth & skeleton)

| When | Accomplishment |
|------|----------------|
| 2026-07 | Module path `spacetimedb/`, flake Rust+wasm+spacetime, Taskfile `stdb:*`, local data under `tmp/spacetimedb` |
| 2026-07 | Domain tables + enums; public `ledger`; separate idempotency table |
| 2026-07 | BitAuth OIDC on Go (`/auth/bitauth/*`); cookies; digitalxero connect smoke |
| 2026-07 | `client_connected` BitAuth policy; `User` PK = Identity; owner in `config` at init |
| 2026-07 | `User.is_admin` + `require_admin`; owner SQL bootstrap for first admin |
| 2026-07 | Documented private-table visibility (host owner vs clients); no issuer hardcode for ops |
| 2026-07-25 | `create_ledger` (admin); `create_account` (debit = any user, credit = admin; Owner perm; address/webhook rules ported from Go) |
| 2026-07-25 | `create_account`: `address: Option` (Some = admin); primary via `user_id` index (`Identity::ZERO` if not primary) |
| 2026-07-25 | `create_transfer` / `finalize_transfer`: pending path; Write+ authz; memo max 32; hash `send\|recv\|amt\|memo\|pending` |

---

## SOURCE OF TRUTH

This document should remain the source of truth during this refactor. If the user presents things contrary to this document or in addition to this document, inform the user of that deviation/addition and **update this document** to reflect their decision. Live schema details in `spacetimedb/src/` win on drift until synced here.
