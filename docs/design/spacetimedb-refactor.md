# Design Doc: SpacetimeDB Refactor

**Status:** Outline + **domain core + webhooks + apps done; account API tokens + module HTTP next** (§7.7–§7.10)  
**Date:** 2026-07-24 (updated 2026-07-29)  
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
4. **Third-party integration paths (two):**
   - **Native STDB clients:** partners/bots connect as **app** Identities (SpacetimeAuth) granted roles on accounts via `account_member` (§7.9).
   - **JSON HTTP API:** account-scoped **API tokens** (`AccountToken`) + **module HTTP handlers** under `/v1/database/:db/route/...` for programmatic account access without a full STDB client (§7.10). Replaces legacy edge `stla_` + JetStream KV + Go `/api`.
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

- Legacy `/login` + `sid` JetStream session path remains until cutover.
- Legacy account API tokens (`stla_` in JetStream KV) / Go JSON `/api` — **replaced by** module `account_token` + HTTP handlers (§7.10); drop edge façade after cutover.
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
| D6 | Third-party API | **Apps (STDB) + account tokens (HTTP)** | Apps: native clients (§7.9). Tokens + module HTTP handlers: JSON programmatic access (§7.10). Replaces legacy `stla_` / edge `/api`. |
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

**Code location:** `spacetimedb/src/` (`lib.rs`, `tables.rs`, `views.rs`, `acl.rs`, `apps.rs`, `transfers.rs`, `webhooks.rs`, planned `api.rs` for tokens + HTTP). Source of truth for live schema is the Rust module; this section summarizes decisions.

### 7.1 Table inventory (current spike schema)

All core tables **private** unless noted. Enums used instead of opaque integer codes where practical.

| Table | Accessor | Purpose | Notes |
|-------|----------|---------|--------|
| `config` | `config` | Singleton owner Identity | Written in `init` from `ctx.sender()` (publisher). PK = `owner` |
| `user` | `user` | Stelo user profile | **PK = `Identity`**. Unique `bitcraft_username`. `is_admin` (default false) |
| `ledger` | `ledger` | Asset type / scale / kind | **Public** catalog. `LedgerKind`: Digital / Derivation / Physical |
| `account` | `account` | Wallet / balances | `AccountKind` Credit/Debit; `user_id` = primary or **`Identity::ZERO`**; multi-col index `by_user_and_ledger`; single-col `ledger_id` + `address` |
| `account_member` | `account_member` (`AccountMember`) | User **or** app ↔ account ACL | `MemberKind` + `Role`; multi-col `by_account_and_member`; single-col `member_id` |
| `account_token` | `account_token` | HTTP API tokens per account | Table stub **started**; reducers + HTTP **in progress** (§7.10). Unique `token`; index `account_id` |
| `app` | `app` | Third-party / bot principal | **PK = Identity** (SpacetimeAuth). Unique `name`. `created_by` human |
| `app_ticket` | `app_ticket` | Pending create/replace | Scheduled TTL ~15m; unique `name` + unique `sub`; see §7.9 |
| `transfer` | `transfer` | Transfer records | `TransferKind` + `TransferState`; optional pending/posted amounts; `finalized_at` optional |
| `transfer_idempotency` | `transfer_idempotency` | Idempotency map | Auto-inc PK; index `by_account_and_key` on `(account_id, key)` |
| `webhook_delivery` | `webhook_delivery` | Webhook outbox = **schedule table** | `scheduled(deliver_webhook)`; see §7.5 / `webhooks.rs` |

**`webhook_delivery` columns:** `id` (PK auto), `scheduled_at` (`ScheduleAt`), `account_id`, `transfer_id`, `url` (snapshot), `payload_json`, `attempts`.

**Public surface:** prefer **views** for balances/PII; public base tables only when intentionally world-readable (`ledger`).

**Private table visibility (platform, not app code):**

- Normal clients (including BitAuth) **cannot** query private tables directly.
- Reducers/views on the server **can** read/write private tables.
- **Database owner** (publisher Identity) may read private tables via `spacetime sql` for debugging.
- `spacetime sql --anonymous` respects client visibility (use to simulate unprivileged clients).

**STDB constraints:** no multi-column unique / composite PK in 2.7; no foreign keys. Uniqueness of composites (address+ledger, account+key, etc.) is **enforced in reducers** when those paths land. Btree indexes support lookups.

### 7.2 Identity, connect policy & admin

```text
BitAuth OIDC (browser / human)
    → Go /auth/bitauth/* (PKCE + client secret)
    → cookie stdb_id_token = OIDC ID token
    → Go connects STDB WithToken(id_token)
    → host verifies JWT, Identity = f(iss, sub)
    → client_connected (lib.rs):
         - if sender == config.owner → allow (ops CLI), no User row
         - else require OIDC JWT
         - resolve OidcProvider from iss + validate aud
         - BitAuth → upsert User { id: sender, bitcraft_username from preferred_username, is_admin: false }
         - SpacetimeAuth → if app row exists allow; else fulfill app_ticket by JWT sub (create/replace app)

App (bot / partner) — SpacetimeAuth anonymous
    → edge: SpacetimeAuth anonymous login → show access + refresh tokens to user
    → edge decodes OIDC sub; human (BitAuth) calls create_app_ticket(name, sub)
    → bot/edge connects WithToken(SpacetimeAuth JWT)
    → client_connected: OidcProvider::SpacetimeAuth → match app_ticket.sub → insert/replace app
    → grant_account_member; bot uses refresh as needed (same sub → same Identity)

Account HTTP API (programmatic JSON) — no STDB Identity for the token holder
    → client calls module HTTP route (Authorization: stla_…)
    → HandlerContext.with_tx looks up account_token; authz is token + account scope (§7.10)
```

**OIDC providers** are modeled as `OidcProvider` in `lib.rs` (`BitAuth` | `SpacetimeAuth`) with `issuer()`, `audience()`, `from_issuer()`, `is_valid_audience()` — replaces loose string constants for iss/aud checks.

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
| **App (third party)** | SpacetimeAuth anonymous JWT → `Identity` → `app` row (via ticket); access via `account_member` |
| **App admin** (Stelo operator) | `User.is_admin == true`; `require_admin(ctx)` on privileged reducers |
| **DB owner** | `config.owner` from `init`; CLI SQL / connect for ops; **not** automatic product admin |
| **Module identity** | `ctx.database_identity()` — scheduling / “is this the module?”, not the publisher |

**Mutual exclusion:** an Identity is never both a `user` and an `app`.

**Admin bootstrap (chicken-and-egg):**

1. Publish module (`init` stores owner).
2. Player logs in via BitAuth → `User` created with `is_admin = false`.
3. Owner runs SQL once:  
   `UPDATE user SET is_admin = true WHERE bitcraft_username = '…';`
4. Later: admin-only reducers may `grant_admin` / `revoke_admin`.

STDB docs’ JWT `roles` claim pattern is preferred if BitAuth ever issues roles; until then **DB flag on `User`**.

Reducers must treat `ctx.sender()` as the principal. Map:

`Identity` → `user` → `account_member` / ownership on `account`.

Edge `ADMIN_KEY` remains temporary break-glass for legacy HTTP only — not module authz long-term.

### 7.3 Views (multi-tenant, caller-dependent)

Views use `ViewContext` / `AnonymousViewContext` and (for per-user views) filter by `ctx.sender()`. Prefer **indexed** `.find()` / `.filter()` only — procedural views **must not** use table `.iter()` (STDB read-set rules). Code: `spacetimedb/src/views.rs`.

**No dedicated `ledger_list` view** — `ledger` is already a public table; clients subscribe/query it directly.

**No parameterized views (STDB 2.7):** the Rust bindings still reject extra args (`TODO: Re-enable parameterized views once we can pass args from sql`). Docs also state views accept only a context parameter. Therefore:

- There is **no** `my_account(account_id)` function-arg view.
- Single-account pages use the multi-row view + **SQL** filter, e.g.  
  `SELECT * FROM my_accounts WHERE account_id = <id>`  
  (same pattern for `my_accounts_members` / `my_transfers`).  
  The host still evaluates the per-user view for that identity; the SQL predicate limits which view rows are returned to the client. This is **not** edge-side over-fetch of other users’ private data — only the caller’s view rows exist. A typical user has few accounts, so this is acceptable until parameterized views return.
- **Do not** add a separate `my_account` view that returns the full set under another name.

**Pagination (investigation, 2026-07-26):**

| Mechanism | Status in 2.7 |
|-----------|----------------|
| View args `limit` / `offset` / cursor | **Not available** (no view parameters) |
| SQL one-off `SELECT … FROM my_transfers LIMIT N` | Likely works for non-subscription queries (confirm when wiring edge); not a true keyset cursor |
| Subscriptions | Generally deliver the **full** current view result for realtime consistency; client-side windowing is OK for UI chrome |
| Future | Re-enable parameterized views; or keyset via indexed `created_at` ranges once args work; or dedicated “page window” tables |

**For now:** views return the full authorized set (no hard LIMIT inside the view). Revisit when transfer history grows.

| View | Returns | Auth / visibility | Status |
|------|---------|-------------------|--------|
| `my_user` | Caller’s profile (`MyUserRow`) | Caller row only; empty/`None` if anonymous/unregistered | **done** |
| `my_accounts` | Accessible accounts: computed `balance` + `kind`, ledger name/scale/kind, caller `role`, `is_primary`, **Owner-role** username (not primary `user_id`), `webhook` only if role ≥ Admin | Via `account_member` for `ctx.sender()`; **not** app-admin god-mode | **done** |
| `my_accounts_members` | Users + apps on accessible accounts (`MemberKind`, display `name`, role) | Caller has **any** role (Read+) on that account | **done** |
| `my_transfers` | Transfers on caller’s accounts; enriched addresses, primary usernames, ledger name/scale | Debit **or** credit account in caller’s ACL; **no dedupe** if both sides match (may emit twice); no view `primary_key` until deduped | **done** |
| `account_directory` | Public: `account_id`, `address`, `ledger_id`, `primary_username` (if `user_id != ZERO`) | **Anonymous OK**; all credit+debit accounts; world-readable addresses | **done** |
| `ledger_audit` | Per-ledger debit-normal vs credit-normal nets + `balanced` | App admin (`User.is_admin`) only; others empty | **done** |
| ~~`my_accounts_users`~~ / ~~`my_accounts_apps`~~ | — | Replaced by `my_accounts_members` | **Removed** |
| `my_account_tokens` | Token metadata for accounts caller Admin+ (id, account_id, created_at; **never** secret) | Caller Admin+ on account | **Planned** (§7.10) |
| ~~`my_account`~~ | — | Use `my_accounts` + SQL `WHERE account_id` | **Skipped** |
| ~~`ledger_list`~~ | — | Public `ledger` table | **Skipped** |

**Row types:** dedicated `*Row` structs (`SpacetimeType`), not raw table types. Prefer `primary_key = …` on multi-row views where stable ids exist (`account_id`, transfer `id`, etc.) for client-cache updates.

**Balance field:** single `balance: u64` from current app semantics (available balance by kind), plus `kind: AccountKind` — **not** the four raw pending/posted counters on the view.

**Realtime:** subscribe to `my_accounts` + `my_transfers` (and optionally `my_accounts_members`) replaces NATS `accounts.transfers.*` for UI.

### 7.4 Reducers (mutations)

All reducers: validate sender, load permission, enforce domain rules, mutate only via this path.

| Reducer / procedure | Responsibility | Status |
|---------------------|----------------|--------|
| `client_connected` | Owner bypass; JWT required; `OidcProvider` BitAuth→user / SpacetimeAuth→app ticket | **done** |
| `require_admin` (helper) | Gate admin reducers on `User.is_admin` | **done** |
| `require_principal` / `effective_role` / `has_minimum_role` (helpers) | User **or** app; role from `account_member` | **done** |
| `create_account_token` / `revoke_account_token` (etc.) | Admin+ manage HTTP API tokens | **Planned** (§7.10 / `api.rs`) |
| `create_ledger` | Admin; public catalog row | **done** |
| `create_account` | Owner = `ctx.sender()`. Debit open; Credit admin; custom address admin; Owner ACL row. **Users only** | **done** |
| `create_transfer` | Kind authz; idempotency; pending/posted; balance rules; webhook enqueue | **done** |
| `finalize_transfer` | Pending only → post amount or void; idempotent replay; webhook enqueue | **done** |
| `grant_account_member` | Upsert by `Identity`; kind from `user`/`app` tables; see §7.8 | **done** |
| `revoke_account_member` | Remove by `Identity`; self-leave OK (non-Owner) | **done** |
| `create_app_ticket` / `replace_app_ticket` | Human; pending SpacetimeAuth `sub` + name; ~15m TTL | **done** |
| `expire_app_ticket` (scheduled) | TTL cleanup when ticket expires | **done** |
| `client_connected` app bind | SpacetimeAuth + ticket by `sub` → create/replace `app` | **done** |
| `set_account_primary` | Owner sets/clears own primary (`account_id`, `bool`); Debit only; **users only** | **done** |
| `set_account_webhook` | Admin+; `Option<String>` (`None`/blank clears); http(s) URL validation | **done** |
| `update_account_address` | App admin; set address by account id (A–Z, unique in ledger) | **done** |
| `admin_patch_balance` | Break-glass; prefer issue/redeem long-term | TODO |
| `grant_admin` / `revoke_admin` | App admin flag after owner-SQL bootstrap | TODO |
| `admin_import_*` | Optional migration helpers | Optional |

**Ledger audit:** multi-tenant/admin **view** `ledger_audit` is **done** (§7.3). No separate `audit_ledger` reducer required unless we want a side-effecting alert path later.

**Role ladder (replaces bitflags `PermAdmin` / `PermReadBal`):** `Read` < `Write` < `Admin` < `Owner` — enum **`Role`**. Membership **`MemberKind`**: User | App on `account_member`. Apps max **Admin**. See **§7.8** / **§7.9**.

**`create_transfer` must preserve:**

- Amount ≥ 1
- Distinct sender/receiver accounts
- Same ledger
- Compatible account kinds → transfer kind (liability / asset / issue / redeem)
- Balance sufficiency (posted path)
- Memo length limits (module: 32; legacy app: 50 — pick one at cutover)
- Idempotency key required; same key + same hash → replay; same key + different hash → conflict
- Atomic balance updates + transfer insert + idempotency insert
- Outbox rows for sender/receiver webhooks if configured (**done**)

### 7.5 Procedures (side effects) & webhook delivery

| Procedure | Role | Status |
|-----------|------|--------|
| `deliver_webhook` (scheduled) | HTTP POST one `WebhookDelivery` job; re-insert with backoff on failure | **done** |

**Architecture (schedule row = job):**

1. `create_transfer` / `finalize_transfer` insert 0–2 `webhook_delivery` rows (`ScheduleAt::Time(now)`) when send/recv accounts have `webhook` set. URL + JSON payload are snapshotted.
2. Host runs `deliver_webhook` when due; **one-shot schedule row is deleted before the procedure runs**.
3. Procedure: scheduler-only guard (`sender == database_identity`); `POST` with 5s timeout (host max 180s); UA `Stelo-Webhooks/1.0`.
4. Success (2xx): done. Failure: if `attempts+1 < 10`, `with_tx` re-inserts job with later `scheduled_at` (backoff); else log and drop (no dead-letter table).

**Payload JSON** (not legacy `code` int):

```json
{
  "id", "debitAccId", "creditAccId", "amount", "ledgerId",
  "kind": "Liability|Asset|Issue|Redeem",
  "state": "Posted|Pending|PostPending|VoidPending",
  "memo", "createdAt": "<RFC3339>"
}
```

`amount` is pending amount for `Pending`, else posted amount. Receivers use `state` to interpret.

Constraints:

- Do not hold a DB transaction open across HTTP.
- Max attempts 10; backoff ≈ 1s, 5s, 30s, 2m, 5m, 15m, 30m, 1h, 2h.
- Enqueue on pending create, posted create, and finalize (void/post).

### 7.6 Accounting invariant

**Hard invariant (per ledger):**

```text
Σ balance(debit-normal accounts) − Σ balance(credit-normal accounts) = 0
```

Balance definition should match current app semantics (posted ± pending as defined today).

Enforcement options (pick during implementation):

1. **Structural:** only `create_transfer` (and tightly controlled admin) mutates balances; unit/integration tests + audit reducer.
2. **Audit:** admin view `ledger_audit` (done); optional later scheduled alert if nets diverge.
3. Avoid unrestricted `admin_patch_balance` in production without paired offsetting entry.

### 7.7 Domain parity matrix (module vs current app)

Source: live Go routes/handlers/SQL/JetStream vs `spacetimedb/`. Goal: **finish module domain surface** before full edge cutover.

#### Already in module

| Area | Status |
|------|--------|
| Auth skeleton | `init`, `client_connected`, `User`, `require_admin` |
| Ledgers | public `ledger` + `create_ledger` |
| Accounts | `account` (+ `webhook` column) + `create_account` |
| ACL rows | `account_member` + Owner on create |
| ACL reducers | `grant_account_member`, `revoke_account_member`, `set_account_primary` (**done**) |
| Transfers | `create_transfer`, `finalize_transfer`, `transfer_idempotency` |
| Webhooks | `set_account_webhook`, `webhook_delivery` schedule table, `deliver_webhook` procedure |
| Pending path | **Beyond** current prod (prod is posted-only) — keep as STDB upgrade |

#### Feature → module surface

| Product feature | Tables | Views | Reducers / procedures |
|-----------------|--------|-------|------------------------|
| Login / user profile | `user` | `my_user` | `client_connected` **done** |
| List my wallets + balances | `account`, `account_member`, `ledger` | `my_accounts` (Owner-role username, not primary) | — |
| Create debit wallet | `account` | — | `create_account` **done** |
| Account settings page | | `my_accounts` (+ SQL filter), `my_accounts_members` | grant/revoke/primary **done**, apps **done** |
| Add/remove members | `account_member` | `my_accounts_members` | `grant_account_member`, `revoke_account_member` **done** |
| Set primary wallet | `account.user_id` | | `set_account_primary` **done** |
| Transfer recipient search | public catalog | `account_directory` | edge LIKE filter |
| Send transfer | `transfer`, balances | `my_transfers` | `create_transfer` **done** + webhook enqueue |
| Pending finalize | | | `finalize_transfer` **done** + webhook enqueue |
| Transfers list / realtime | | `my_transfers` subscribe | replaces NATS subjects |
| Third-party bots / apps | `app`, `account_member`, `app_ticket` | `my_accounts_members` | tickets + grant/revoke **done** (§7.9) |
| Account API tokens + JSON HTTP | `account_token` (stub) | `my_account_tokens` planned | reducers + HTTP handlers **in progress** (§7.10) |
| Legacy edge JSON `/api` + JetStream `stla_` | — | — | Superseded by §7.10; drop after cutover |
| Webhook URL CRUD | `account.webhook` | field on account view | `set_account_webhook` **done** |
| Webhook delivery | **`webhook_delivery` schedule table** | — | enqueue + `deliver_webhook` **done** |
| Public ledgers | `ledger` public **done** | (table itself; no view) | `create_ledger` **done** |
| Ledger audit | `account` | `ledger_audit` **done** | — |
| Admin create credit / custom addr | | | `create_account` **done** |
| Admin address / balance patch | | | `update_account_address` **done**; `admin_patch_balance` TODO |
| Admin user lookup | `user` | optional | optional |

#### Module build order (STDB-first)

1. ~~**Views:**~~ **done** (`views.rs`). Isolation tests optional/skipped for speed.
2. ~~**ACL reducers:**~~ **done** (`acl.rs` + §7.8).
3. ~~**Webhook config:**~~ **done** (`set_account_webhook`).
4. ~~**Outbox + deliver:**~~ **done** (`webhook_delivery` + `deliver_webhook`).
5. ~~**Apps + unified members:**~~ **done** (`app` / `account_member` / `app_ticket`, SpacetimeAuth bind, grant/revoke, views) — §7.9.
6. **Account API tokens + module HTTP:** `account_token` reducers (`api.rs`), public `GET /ping`, token-gated account ping — §7.10 (**next**).
7. **Admin:** `grant_admin` / `revoke_admin`, optional `admin_patch_balance` (`update_account_address` + `ledger_audit` already exist).
8. **Hardening:** invariant tests, second-identity view isolation, webhook retry/dead-letter, app smoke (see §7.9 testing notes); restore Credit↔Credit (`Liability`) arm if still missing in `identify_transfer_kind`.

#### Explicit non-goals / accepted differences

| Topic | Note |
|-------|------|
| Pending transfers | In module; not in production `CreateTransfer` — keep |
| Bitflag permissions | Role enum is enough; UI only uses Admin today |
| NATS permission events | Drop if views+subscribe cover UI |
| JetStream user sessions (`sid`) | Edge/OIDC (P1), not module domain |
| `ADMIN_KEY` HTTP header | → `User.is_admin` + admin reducers |
| Parameterized fuzzy search in-view | Public `account_directory` + edge filter |
| Memo length | Module 32 vs legacy 50 — decide at API cutover |
| STDB HTTP path params `{id}` | **Not in 2.7** — exact paths only; see §7.10 route design |

### 7.8 Account ACL rules (`grant` / `revoke` / `primary`)

Authoritative product rules (implemented in `spacetimedb/src/acl.rs` on unified `account_member`):

**Invariants**

- Exactly **one** `Role::Owner` per account (created on `create_account`; preserved on grant/revoke). Count by `role == Owner` (apps cannot be Owner).
- `account.user_id` (primary) is either **`Identity::ZERO`** or the **Owner’s** identity — never a non-Owner.
- Only **Debit** accounts may be primary. One primary per `(user, ledger)`.
- No app-admin god-mode on these reducers (account ACL only). Credit accounts use the same ACL rules (primary still Debit-only).
- Callers may be **users or apps** (`require_principal` + `effective_role`) except `set_account_primary` / `create_account` (humans only).

**`grant_account_member(account_id, member_id: Identity, role)`**

| Rule | Detail |
|------|--------|
| Caller | Admin+ on the account |
| Target | Must exist in `user` **or** `app` (kind resolved from tables; user wins if both — should not happen) |
| Self-grant | **No** |
| Apps | Max Admin; **cannot** be Owner |
| Admin may assign | Read, Write, Admin — **not** Owner; cannot modify Owner’s row |
| Owner may assign | Read, Write, Admin, Owner (**Owner only to users**) |
| Owner transfer | Grant Owner to another user: demote caller → Admin; promote target → Owner; requires **`account.user_id == ZERO`** |
| Demote Owner | Not via grant except transfer path; hard-fail if 0 Owners remain |

**`revoke_account_member(account_id, member_id: Identity)`**

| Rule | Detail |
|------|--------|
| Leave (self) | Any role **except Owner** may revoke **themselves** |
| Owner | May revoke any other member; **cannot** revoke self |
| Admin | May revoke anyone **except Owner** |
| Read/Write | May only revoke self |
| Owner row | Never deleted by revoke — transfer ownership first |

**`set_account_primary(account_id, primary: bool)`**

| Rule | Detail |
|------|--------|
| Caller | Must be **Owner** (no user arg — always `ctx.sender()`) |
| `primary = true` | Account must be Debit; caller has no other primary on this ledger; set `user_id = caller` |
| `primary = false` | Clear `user_id` to `ZERO` (caller must be current primary) |

### 7.9 Apps (third-party STDB clients)

**Code:** `spacetimedb/src/apps.rs` (tickets + connect bind), `acl.rs` (`grant_account_member` / `revoke_account_member`), `tables.rs` (`app`, `account_member`, `AppTicketPurpose`), `lib.rs` (`client_connected`).

**IdP:** SpacetimeAuth anonymous (`iss` `https://auth.spacetimedb.com/oidc`, `aud` = project client id). Tokens (access + refresh) are issued by SpacetimeAuth and shown on the edge — **never stored in the module**. Module config is `OidcProvider::SpacetimeAuth` (`issuer` / `audience` in `lib.rs`) — must match the real SpacetimeAuth project client.

**Tables**

```text
app
  id           Identity PK     -- SpacetimeAuth-derived principal
  name         String unique
  created_by   Identity
  created_at, updated_at

account_member  (users + apps)
  id           u64 PK auto_inc
  account_id   u64
  member_id    Identity        -- btree + multi-col by_account_and_member
  kind         MemberKind      -- User | App
  role         Role            -- apps: max Admin
  created_at, updated_at

app_ticket  (scheduled expire_app_ticket, at = expires_at)
  id           u64 PK auto_inc
  expires_at   ScheduleAt      -- ~15 minutes from create
  created_by   Identity
  name         String unique   -- Create: new name; Replace: existing app name
  sub          String unique   -- SpacetimeAuth OIDC sub
  purpose      AppTicketPurpose  -- Create | Replace
  created_at
```

**Create flow**

1. Edge: SpacetimeAuth anonymous login → show tokens; decode JWT `sub`.
2. Human (BitAuth) calls `create_app_ticket(name, sub)`.
3. Ticket rules: name free of existing **apps** and of **other users’ tickets**; same user re-creating same name **replaces** their ticket; `sub` unique (same user replaces prior ticket on that sub).
4. Connect with SpacetimeAuth JWT → `client_connected` finds ticket by `sub` → insert `app`, delete ticket.
5. No ticket for that SpacetimeAuth identity → **reject connect**.

**Replace identity**

1. Owner calls `replace_app_ticket(name, new_sub)` (app must exist; caller = `created_by`).
2. Name may match existing app (not a “name taken” for Create uniqueness against apps).
3. Connect with new SpacetimeAuth token → migrate `account_member` rows (`member_id`) to new Identity; swap `app` row; delete ticket.

**Grant / revoke apps:** use `grant_account_member` / `revoke_account_member` with the app’s Identity (`MemberKind::App` resolved from `app` table).

**Authz wiring:** `effective_role` = single `account_member` lookup for `ctx.sender()`.

**Later (not v1):** edge SpacetimeAuth UI; partner docs; rename/delete app; rate limits.

**Testing notes (defer implementation; exercise later)**

- `create_app_ticket` / name uniqueness / same-user ticket replace / other-user name taken.
- Ticket TTL expires (~15m).
- SpacetimeAuth connect with matching ticket creates app; without ticket rejects.
- Replace migrates memberships; old identity no longer has app row.
- User Identity ≠ app Identity mutual exclusion.
- Grant/revoke matrix; reject Owner for apps; unified Identity API.
- App Write can transfer; App Read cannot; Admin can grant other members.
- Views: Read+ sees `my_accounts_members`; isolation across accounts.
- `create_account` / `set_account_primary` denied for apps.

### 7.10 Account API tokens + module HTTP handlers

**Decision (2026-07-29):** Re-open account API tokens for **JSON HTTP** programmatic access. Apps (§7.9) remain the path for **native STDB clients**. Tokens are **not** SpacetimeDB Identities — they authenticate HTTP handlers only.

**Code:** table stub in `tables.rs` (`AccountToken`); reducers + HTTP in planned `spacetimedb/src/api.rs`; enable `spacetimedb` Cargo feature **`unstable`** (HTTP handlers are beta).

#### Table (`account_token`)

Current stub:

```text
account_token
  id           u64 PK auto_inc
  account_id   u64  index btree
  token        String unique   -- secret material (see hashing note)
  created_at   Timestamp
```

**Likely additions before ship:** `created_by: Identity`; optional display `name`/`label`; consider storing **hash only** (return plaintext once on create) instead of unique plaintext `token` — decide at implement time. Tokens are full **Admin-equivalent for that account’s HTTP surface** (matches legacy: one token type with admin access to the account).

#### Token management reducers (BitAuth / STDB session callers)

| Reducer | Authz | Behavior |
|---------|-------|----------|
| `create_account_token(account_id, …)` | **Admin+** on account (`require_account_role` Admin) | Generate secret (`stla_` + random); insert row; **return secret once** to caller (reducer return or side-channel — reducers traditionally `Result<(), String>`; may need procedure or return `String` if bindings allow) |
| `revoke_account_token(account_id, token_id)` | Admin+ | Delete one token by id (must belong to account) |
| `revoke_all_account_tokens(account_id)` | Admin+ | Delete all tokens for account (legacy delete-all) |

View `my_account_tokens`: metadata only for accounts where caller is Admin+; **never** the secret.

#### Module HTTP surface (STDB handlers)

Public URL shape (host):

```text
$STDB_URI/v1/database/$DATABASE/route<path>
```

Handlers use `#[spacetimedb::http::handler]` + `#[spacetimedb::http::router]`. `HandlerContext` has **no caller Identity** for custom tokens — use `with_tx` / `try_with_tx` (tx sender is `Identity::ZERO`) and authorize by looking up `account_token` from the `Authorization` header.

**Route path constraints (STDB 2.7):** exact match only; path chars = **lowercase ASCII, digits, `-_~/`**. No `{param}` / `:param` / wildcards yet (platform reserved for future). Trailing slashes are significant.

| Route (module path) | Auth | Response | Notes |
|---------------------|------|----------|--------|
| `GET /ping` | None | `200` body `pong` | Health / smoke |
| Account ping | Valid `account_token` for that account | `200` body `pong` | See path design below |

**Path design for account-scoped routes (until path params exist):**

Preferred v1 (works with exact routes):

| Approach | Path | How account is bound |
|----------|------|----------------------|
| **A (recommended)** | `GET /accounts/ping` | Token row’s `account_id` only; optional query `account_id=` must match token if present |
| **B (closer to legacy docs)** | Wait for STDB path params | Then `GET /accounts/{id}/ping` + token must match `{id}` |

Do **not** invent per-id static routes. Document public API as legacy-shaped when path params land; implement A until then.

Auth header (legacy-compatible):

```text
Authorization: stla_<secret>
```

(or raw secret if we drop the prefix — prefer keep `stla_` for drop-in familiarity).

#### Relation to apps / edge

| Path | Use when |
|------|----------|
| **Apps + STDB SDK** | Realtime subscribe, reducers, same authz as humans |
| **Account tokens + HTTP** | Simple JSON/HTTP integrations, scripts, no STDB client |
| **Go edge `/api`** | Temporary façade until module HTTP is complete; then deprecate |

#### Initial implementation slice

1. Enable `unstable` on `spacetimedb` dependency.
2. Flesh `AccountToken` + create/revoke reducers in `api.rs`.
3. `GET /ping` → `pong`.
4. `GET /accounts/ping` → require token → `pong`.
5. Smoke with `curl` against local STDB `/v1/database/stelofinance/route/...`.

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

- **Module-native** via `account_token` + HTTP handlers (§7.10). Edge may still mint/proxy legacy JetStream `stla_` until cutover; long-term mint is a reducer (Admin+), call is direct to STDB `/route/...`.

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

**Target:** module HTTP handlers (§7.10) under `/v1/database/:db/route/...`, not a permanent Go re-implementation of domain routes.

| Area | Strategy |
|------|----------|
| Ledgers list/create/audit | Views + admin reducers (edge or STDB client); HTTP later if needed |
| Accounts search / get | Views / future HTTP |
| Transfers list/get/create | Views + `create_transfer` / future HTTP |
| Webhook get/put/delete | Views + reducers / future HTTP |
| Ping (public + account) | Module HTTP **first slice** (`/ping`, `/accounts/ping`) |

During cutover, Go `/api` may façade STDB; after module HTTP covers the docs surface, drop edge domain routes.

Breaking changes: minimize; if STDB IDs differ from old SQLite integers, document migration (u64 vs int64) and update docs. Path shape may differ from legacy `/api/accounts/{id}/…` until STDB supports path parameters (§7.10).

### 9.2 Direct STDB clients (apps)

Partners use official SDKs (TS/Rust/C#) with **SpacetimeAuth anonymous** tokens (after `create_app_ticket` + first connect bind), and an account Admin+ `grant_account_member` with the app Identity. Same reducers/views as humans (`effective_role`). Edge is optional for them. See §7.9.

### 9.3 Account HTTP tokens

Partners that want plain HTTP/JSON (no STDB SDK) use **account API tokens** + module routes (§7.10). Orthogonal to apps: same account may have both members (apps/users) and HTTP tokens.

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
create_transfer / finalize_transfer (single tx)
  → insert/update transfer + balances (+ idempotency on create)
  → if send/recv account.webhook set: insert webhook_delivery schedule row(s)
       (ScheduleAt::Time(now); URL + payload_json snapshotted)

deliver_webhook procedure (host deletes one-shot schedule row first)
  → scheduler-only guard (sender == database_identity)
  → HTTP POST JSON body (no open TX)
  → success 2xx: done
  → failure: with_tx re-insert job with attempts+1 and later scheduled_at
             (or drop after max 10 attempts; no dead-letter table)
```

Details: §7.5 / `spacetimedb/src/webhooks.rs`.

### 11.2 Payload compatibility

STDB module payload (breaking vs legacy `code` int — documented in `docs/api/webhooks.md`):

`id`, `debitAccId`, `creditAccId`, `amount`, `ledgerId`, **`kind`** (string), **`state`** (string), `memo`, `createdAt` (RFC3339).

`amount` is the pending amount when `state == Pending`, otherwise the posted amount.

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
5. Check `ledger_audit` (or equivalent) for every ledger.
6. Notify testers; optional re-login via OIDC (sessions not migrated).

---

## 13. Open questions / follow-ups

| ID | Topic | Status | Notes |
|----|-------|--------|-------|
| Q1 | Full threat model (abuse, spam transfers, energy, anonymous connect policy) | **Follow up later** | Issuer/aud gate exists for BitAuth; expand rate limits etc. |
| Q2 | Cookie details: name, Max-Age, rotation, logout | **Mostly decided** | `stdb_id_token` / `stdb_refresh_token`; refresh rotation & revoke TBD |
| Q3 | OIDC claim mapping | **Decided (dev)** | Stable `sub` = player id; `preferred_username` = display; see §7.2 |
| Q4 | Account API token validation path | **Decided (re-open)** | Module `account_token` + HTTP handlers (§7.10); apps still for native STDB (§7.9) |
| Q5 | Exact table/view/reducer names | **In flux** | Live schema in `spacetimedb/src/` (`tables.rs`, `views.rs`, …) |
| Q6 | Pending transfer flags / states | **Decided** | `TransferState` + optional pending/posted amounts; `create_transfer(pending)` + `finalize_transfer`; webhooks include `state` + amount semantics (§7.5) |
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
    src/lib.rs            # init, client_connected, OidcProvider, create_ledger/account, role helpers
    src/tables.rs         # domain schema (user, app, account, account_member, account_token, transfer, …)
    src/views.rs          # multi-tenant + public views
    src/acl.rs            # grant/revoke user+app, primary, webhook
    src/apps.rs           # app_ticket + create/replace tickets; connect-time bind
    src/api.rs            # account_token reducers + HTTP router/handlers (planned)
    src/transfers.rs      # create_transfer, finalize_transfer
    src/webhooks.rs       # webhook_delivery schedule table + deliver_webhook
    Cargo.toml            # spacetimedb features include unstable (HTTP handlers)
  spacetime.json          # database + module-path
  spacetime.dev.json      # server: local (committed shared dev)
  # spacetime.local.json  # personal overrides — gitignored
  scripts/seed-hexcoin    # local seed helper
  cmd/app/
  internal/bitauth/       # OIDC client (go-oidc)
  internal/stdb/          # ConnectOnce helper (digitalxero)
  internal/handlers/      # bitauth routes + legacy
  Taskfile.yml            # stdb:start | publish | live | logs | reset | seed
  tmp/spacetimedb/        # local STDB data-dir (gitignored via tmp/)
  docs/design/spacetimedb-refactor.md
  docs/api/webhooks.md    # public webhook payload contract
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
- [x] Seed script `scripts/seed-hexcoin` (admin JWT reducers + owner SQL for private IDs)
- [x] Multi-tenant views: `my_user`, `my_accounts`, `my_transfers`, `my_accounts_members`, `account_directory`, `ledger_audit` (see §7.3 / `views.rs`)
- [ ] Prove a second identity cannot read the first identity’s view data
- [ ] Go edge: one-off query views / reducer call beyond smoke
- [ ] One app page rendered from STDB (parallel to legacy `/app`)
- [ ] SSE + subscribe → Datastar patch on transfer

---

## 19. Appendix A — Current → target mapping

| Current component | Target |
|-------------------|--------|
| `gensql` models | Rust tables + generated Go types |
| `accounts.CreateTransfer` | `create_transfer` reducer |
| `accounts.EventTransfer` + NATS publish | STDB row updates + optional outbox |
| JetStream sessions KV | STDB token cookie (+ module user row) |
| JetStream account tokens | **`account_token` table + module HTTP** (§7.10); apps remain for STDB SDK clients |
| JetStream webhook stream | `webhook_delivery` schedule table + `deliver_webhook` procedure |
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
4. ~~`create_transfer` / `finalize_transfer` + seed script~~ **done**.
5. ~~Domain parity matrix documented (§7.7)~~ **done**.
6. ~~Implement multi-tenant views (§7.3)~~ **done** (`spacetimedb/src/views.rs`).
7. ~~ACL reducers (`grant` / `revoke` / `set_primary`)~~ **done** (§7.8 / `acl.rs`).
8. ~~Webhook config (`set_account_webhook`)~~ **done**.
9. ~~Outbox + `deliver_webhook`~~ **done**.
10. ~~**Apps** (SpacetimeAuth tickets + `account_member`, views)~~ **done** (§7.9).
11. **Next:** account API tokens + module HTTP (`api.rs`, §7.10) — table stub started; reducers + `/ping` + token-gated account ping.
12. Admin reducers (`grant_admin` / `revoke_admin`, optional `admin_patch_balance`); `update_account_address` **done**.
13. Wire Go page off STDB + Datastar subscribe (still P0 exit); edge UI for apps later.
14. Cut over `/app` session from JetStream `sid` to BitAuth/STDB when ready (P1).
15. After fuller P0, expand this doc into an **implementation spec** (exact reducer signatures, error codes, cookie RFC polish).

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
| 2026-07-26 | Seed script `scripts/seed-hexcoin` (hexcoin ×3 Physical, STELOBANK + 10 debits, issue 100_000 each) |
| 2026-07-26 | Documented full domain parity matrix (§7.7): views, remaining reducers, `account_token` / `webhook_delivery`, procedures, build order |
| 2026-07-26 | Doc hygiene: rename outbox→`webhook_delivery`, repo layout, Q6 pending finalized, drop `audit_ledger` reducer TODO (view sufficient) |
| 2026-07-26 | Views: `my_user`, `my_accounts`, `my_accounts_users`, `my_transfers`, `account_directory`, `ledger_audit` in `views.rs`; no `my_account` (SQL WHERE on multi-row views); pagination notes in §7.3 |
| 2026-07-26 | `my_accounts.owner_username` = Owner-role ACL user; rename `my_account_users` → `my_accounts_users`; `ledger_audit` uses shared `is_admin` |
| 2026-07-26 | ACL: `grant_account_user`, `revoke_account_user`, `set_account_primary` + §7.8 rules (`acl.rs`) |
| 2026-07-26 | `set_account_webhook(account_id, Option<String>)` Admin+; reuses `normalize_webhook` |
| 2026-07-26 | Webhook outbox: `webhook_delivery` schedule table + `deliver_webhook` procedure; enqueue on create/finalize; payload `kind`/`state` strings |
| 2026-07-26 | **Apps:** cancel account API tokens / third-party JSON API; `Role` rename; `app` + `account_app`; `grant`/`revoke_account_app`; `effective_role`; `my_accounts_apps` (§7.9) |
| 2026-07-27 | Apps auth: drop procedure identity mint; SpacetimeAuth tickets (`app_ticket` scheduled, `create_app_ticket` / `replace_app_ticket`); connect-time fulfill by OIDC `sub`; webhook_delivery PK → `id` |
| 2026-07-28 | Merge `account_user` + `account_app` → `account_member` + `MemberKind`; `grant`/`revoke_account_member(Identity)`; `my_accounts_members` replaces users/apps views |
| 2026-07-28 | Cleanup `client_connected`: `OidcProvider` enum (BitAuth / SpacetimeAuth iss+aud); JWT-first then provider branch |
| 2026-07-28 | Module cleanup: drop `is_admin()` helper (use `user.is_admin`); rename `has_account_role` → `has_minimum_role`; tighten `grant_account_member` / webhook enqueue comments |
| 2026-07-29 | Views: inline Admin\|Owner for webhook field; remove `role_is_admin_plus` |
| 2026-07-29 | **Re-open account API tokens:** `AccountToken` table stub; plan module HTTP (`api.rs`) + reverse D6/Q4 “won’t do” (§7.10) |

---

## SOURCE OF TRUTH

This document should remain the source of truth during this refactor. If the user presents things contrary to this document or in addition to this document, inform the user of that deviation/addition and **update this document** to reflect their decision. Live schema details in `spacetimedb/src/` win on drift until synced here.
