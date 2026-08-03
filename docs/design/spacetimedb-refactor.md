# Design Doc: SpacetimeDB Refactor

**Status:** Outline + **domain core + webhooks + apps + account HTTP tokens done; Topcoat edge migration inventory next** (§7.7–§7.10, §8)  
**Date:** 2026-07-24 (updated 2026-08-02)  
**Author:** Stelo maintainers + design discussion  
**Related:** Current stack is Go + SQLite (sqlc/goose) + embedded NATS/JetStream + Datastar; target edge is **Rust Topcoat** + first-party STDB client; module + BitAuth remain

---

## 1. Summary

Replace SQLite and NATS with **SpacetimeDB** as the single system of record, application logic host, realtime sync layer, and (eventually) direct third-party client surface.

Replace the **Go** lite webserver with a **Rust Topcoat** edge so the site uses SpacetimeDB’s **first-party Rust client SDK** (not the Go third-party client).

Split the product into two deployables:

| Piece | Role |
|-------|------|
| **SpacetimeDB module** (Rust) | Tables, reducers, views, procedures. All domain/financial logic. Module HTTP under host routes. |
| **Lite webserver** (Rust / [Topcoat](https://github.com/tokio-rs/topcoat)) | HTTP/HTML bridge. BitAuth OIDC → cookie holding STDB token. Datastar patches via Topcoat. STDB client (official Rust) as the user. **HTTP reverse-proxy** in front of module routes (our domain + path reshape). **No domain rules.** |

Production module runs on **SpacetimeDB mainnet/maincloud**. Local dev runs a local SpacetimeDB instance. Edge is **stateless** on Fly (no SQLite/NATS volume). Existing production balances are small/tester-only; a **manual data import** cutover is acceptable.

---

## 2. Goals

1. **Single source of truth** for accounts, balances, transfers, permissions, tokens, webhooks.
2. **All money-moving and authz rules live in the module** (reducers + private tables + views).
3. **Realtime UI** without NATS: STDB subscriptions → edge re-renders Datastar HTML fragments.
4. **Third-party integration paths (two):**
   - **Native STDB clients:** partners/bots connect as **app** Identities (SpacetimeAuth) granted roles on accounts via `account_member` (§7.9).
   - **JSON HTTP API:** account-scoped **API tokens** (`AccountToken`) + **module HTTP handlers**; edge **reverse-proxies** them on our domain with a **new path shape** (not legacy Go `/api/...`). Path params and other shapes STDB lacks can be expressed on the edge and mapped onto module routes (§7.10, §8.5, §9).
5. **Browser acts as an authenticated STDB identity**, with the lite server connecting as that user (token-in-cookie), not as a privileged superuser.
6. Remove operational dependency on embedded JetStream (sessions KV, transfer pub/sub, webhook work queue) **and on the Go edge entirely** once Topcoat is cut over.
7. **First-party STDB client on the edge** (Rust SDK + generated bindings) for type-safe queries/reducers/subscriptions.

## 3. Non-goals (this refactor)

- Perfect zero-downtime dual-write migration (testers can be re-imported).
- Full threat-model / abuse / rate-limit design (tracked as follow-up).
- Per-user SpacetimeDB databases (we use **one multi-tenant module**).
- Preserving legacy Go `/api/...` URL compatibility for third parties (new API shape; document the break).
- Keeping BitJita / JetStream `sid` login (hard drop; BitAuth only).
- Edge break-glass `ADMIN_KEY` (product admin is `User.is_admin` only).
- Per-page Tailwind split-build / cache-aware CSS serving on day one (desired later; §8.4 F3).

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
  │  cookie: stdb_id_token (+ optional stdb_refresh_token)
  ▼
Lite webserver (Rust / Topcoat)  ── acts as that user ──►  SpacetimeDB (mainnet / local)
  │   - HTTP routes (module auto-discovery)                    │
  │   - BitAuth OIDC cookies                                   │  Rust module:
  │   - Topcoat view! + Datastar                               │  private tables
  │   - Official STDB Rust client + Identity pool              │  reducers, views
  │   - Reverse-proxy → module HTTP (our domain + paths)       │  HTTP handlers
  │   - NO balance/transfer/authz rules                        │  webhook procedures
  ▼
Static assets (CSS/JS/fonts via Topcoat asset pipeline)
```

**Cutover note:** The Go edge (`cmd/app`, `web/`, `internal/*`) remains the live server until Topcoat reaches parity. New edge work lands under the workspace crate root (`src/`, `Cargo.toml` — already a Topcoat spike). BitJita `/login` + JetStream `sid` are **not** ported.

### 5.1 Responsibility split

| Layer | Owns | Does not own |
|-------|------|--------------|
| **Module (Rust)** | Schema, mutations, authz, invariants, outbox/webhooks, public read surface via views, module HTTP handlers | HTML, cookies, OIDC browser redirect UX, public URL shape of the JSON API |
| **Lite edge (Topcoat/Rust)** | OIDC login UX, cookie storage of STDB token, HTML/Datastar rendering, STDB client I/O as user, reverse-proxy of module HTTP onto our domain | Whether a transfer is valid; what rows a user may see; long-term product admin (`User.is_admin` in module) |

**Rule of thumb:** if the answer affects money or privacy, it belongs in the module.

### 5.2 “Edge as the user” (session model)

**Decision:** **BitAuth OIDC only** (`https://auth.trinit.is/`, BitCraft sign-in). No BitJita. Store the OIDC **ID token** in an HTTP-only cookie. The lite webserver uses that cookie to open or **reuse a pooled** STDB connection **as that identity**.

Desired properties:

- Edge is **not** a god-mode service identity for user reads/writes.
- Module authorization is based on `ctx.sender()` (STDB `Identity`), so direct clients reuse the same reducers/views.
- Page render path: **one STDB session / identity context** loads the data needed for the template (via one-off queries or short-lived subscribe), rather than many ad-hoc DB round-trips. Prefer few views that return render-ready shapes.
- **Auto-refresh** ID token from `stdb_refresh_token` when near expiry (best effort), before STDB connect when possible.

**Target edge auth (Topcoat):**

| Item | Choice |
|------|--------|
| IdP | BitAuth — `https://auth.trinit.is/` (OIDC Auth Code + PKCE, confidential client) |
| Edge OIDC | Rust OIDC stack (library TBD at implementation; same flow as current Go spike) |
| Cookie (ID token for STDB) | `stdb_id_token` (HttpOnly, SameSite=Lax; Max-Age from JWT `exp`) |
| Cookie (refresh) | `stdb_refresh_token` when `offline_access` granted (~14d BitAuth); used for auto-refresh |
| Cookie Secure flag | `BITAUTH_SECURE_COOKIES` / prod; local HTTP often `false` |
| Login UX | Login page with **“Login with BitAuth”** button → authorize URL |
| Login routes | `/auth/bitauth/login`, `/callback`, `/logout` (+ optional `/session` debug) |
| STDB client | **Official Rust SpacetimeDB SDK** + `spacetime generate` bindings |
| Do not | Overwrite `stdb_id_token` with short-lived websocket tokens returned on connect |
| Drop | BitJita `/login`, JetStream `sid`, edge `ADMIN_KEY` break-glass |

**Go spike (historical, still in tree until cutover):** parallel `/auth/bitauth/*` routes + digitalxero connect smoke. Not the long-term edge.

### 5.3 Connection model

| Phase | Behavior |
|-------|----------|
| **After connect works** | **Per-Identity connection pool** — first milestone after basic connect-as-user. Reuse open client connections across page navigations for the same Identity; avoid connect/teardown per request. |
| **SSE / Datastar streams** | May pin a longer-lived connection for the duration of the stream (pool-aware acquire/release). |
| **Cold path** | If no pooled conn: connect with cookie token → use → return to pool (or drop if pool full / idle TTL). |

Pool key is **STDB Identity** (or stable token identity), not OIDC client_id. Eviction/idle TTL/max size are implementation details.

---

## 6. Decisions log

| # | Decision | Choice | Notes |
|---|----------|--------|--------|
| D1 | Module language | **Rust** | Error handling + type system for financial logic |
| D2 | Lite edge language | **Rust + Topcoat** | Replaces Go edge; first-party STDB client; Datastar/Tailwind/assets via Topcoat |
| D3 | Data access control | **Private tables + public views + sender-checked reducers** | Design as if clients connect directly from day one |
| D4 | Webhooks | **Outbox table + scheduled procedure (HTTP from module)** | Replaces JetStream work queue |
| D5 | Browser session | **BitAuth OIDC → STDB token in cookie; edge calls STDB as user** | Avoid privileged edge for user data; BitJita hard-dropped |
| D6 | Third-party API | **Apps (STDB) + account tokens (HTTP)** | Module handlers + **edge reverse-proxy** on our domain (§8.5, §9). New path shape (not legacy `/api`). |
| D7 | Multi-tenancy | **Single module, multi-tenant views** | View output depends on caller identity |
| D8 | Hosting | **Mainnet/maincloud prod; local STDB for dev** | Edge: **stateless Fly** (no volume); GHA deploys module + edge |
| D9 | Migration | **Manual/import cutover OK** | Tester-scale production data |
| D10 | Edge STDB access | **Official Rust SDK + `spacetime generate` bindings** | Replaces digitalxero / Go codegen story |
| D11 | Edge rewrite rationale | **First-party client + Topcoat stack** | Typed client and long-term maintainability; not “just query builders” |
| D12 | Module path | **`spacetimedb/`** (CLI default) | Not `module/`; `spacetime.json` `module-path` |
| D13 | Local STDB config | **`spacetime.json` + `spacetime.dev.json`** | Dev: `server: local`, DB name `stelofinance`; data dir `tmp/spacetimedb` |
| D14 | Browser IdP | **BitAuth only** (`auth.trinit.is`) | Auth Code + PKCE; confidential client secret on **edge only** |
| D15 | STDB principal | **`Identity` = f(iss, sub)** as `User` PK | BitAuth `sub` is stable numeric player id; username is `preferred_username` |
| D16 | User bootstrap | **`client_connected` only** (no separate `ensure_user`) | Upsert on connect; no JWT → reject (except owner) |
| D17 | App admin | **`User.is_admin: bool`** + `require_admin` | Bootstrap first admin via owner SQL; **no edge `ADMIN_KEY`** |
| D18 | DB owner / CLI | **Store owner in `config` at `init`** | Owner may connect for SQL without BitAuth; not product admin |
| D19 | Private tables | **Default private; public only `ledger` (catalog)** | Host enforces client visibility; owner SQL can read private |
| D20 | Composite uniqueness | **Reducer-enforced** (STDB 2.7 has no multi-col unique) | Indexes for lookup (e.g. idempotency `by_account_and_key`) |
| D21 | Idempotency storage | **Separate `transfer_idempotency` table** | Scope `(account_id, key)` → transfer + request_hash |
| D22 | ~~Go STDB client~~ | **Superseded by D10** | Go digitalxero spike remains only until edge cutover |
| D23 | Connection pooling | **Per-Identity pool — first milestone after connect** | Not optional later; see §5.3 |
| D24 | Account create authz | **Debit open; Credit admin-only; custom address admin-only** | Maps former GA vs SRA/PRA; owner is always `ctx.sender()` |
| D25 | Primary account `user_id` | **`Identity` + `ZERO` sentinel** (not `Option`) | Enables `user_id` index filter for “one primary per user per ledger” |
| D26 | Edge framework | **Topcoat** (`tokio-rs/topcoat`) | Module route auto-discovery; built-in Datastar, Tailwind, assets, icons, fonts |
| D27 | Edge routing | **Topcoat module auto-discovery** | Prefer `Router::builder().discover()` over hand-rolled route tables |
| D28 | Browser login UX | **Login page → “Login with BitAuth”** | Hard drop BitJita chat handshake |
| D29 | Token refresh | **Auto-refresh near expiry** | Use `stdb_refresh_token` when present; best effort before STDB connect |
| D30 | Edge error UX | **Map errors to toasts or full error pages** | Depending on severity / request type (HTML vs Datastar vs proxy JSON) |
| D31 | Edge logging | **stdout** (structured if easy) | No NATS log bus; no remote log-level subscribe |
| D32 | Analytics | **Drop PostHog** | Not ported to Topcoat |
| D33 | Public API surface | **Edge reverse-proxy of module HTTP** | Our domain; edge may reshape paths (e.g. path params); forwards `Authorization` |
| D34 | Deploy | **GHA → STDB publish + Fly container (Rust binary)** | Stateless edge; no Fly volume for SQLite/NATS |

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
| `account_token` | `account_token` | HTTP API tokens per account | **done** (§7.10): secret plaintext unique, `label`, `created_by`; index `account_id` |
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
    → Topcoat /auth/bitauth/* (PKCE + client secret)
    → cookie stdb_id_token = OIDC ID token (+ optional refresh)
    → edge connects STDB WithToken(id_token) via Identity pool
    → host verifies JWT, Identity = f(iss, sub)
    → client_connected (lib.rs):
         - if sender == config.owner → allow (ops CLI), no User row
         - else require OIDC JWT
         - resolve OidcProvider from iss + validate aud
         - BitAuth → upsert User { id: sender, bitcraft_username from preferred_username, is_admin: false }
         - SpacetimeAuth → if app row exists allow; else fulfill app_ticket by JWT sub (create/replace app)

App (bot / partner) — SpacetimeAuth anonymous
    → edge or partner: SpacetimeAuth anonymous login → access + refresh tokens
    → human (BitAuth) calls create_app_ticket(name, sub)
    → bot connects WithToken(SpacetimeAuth JWT)
    → client_connected: OidcProvider::SpacetimeAuth → match app_ticket.sub → insert/replace app
    → grant_account_member; bot uses refresh as needed (same sub → same Identity)

Account HTTP API (programmatic JSON) — no STDB Identity for the token holder
    → client → edge reverse-proxy (our domain) → module HTTP
    → Authorization header forwarded; HandlerContext.with_tx looks up account_token (§7.10, §8.5)
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

No edge `ADMIN_KEY` / break-glass. Product admin is **`User.is_admin`** only (owner SQL bootstrap, then admin reducers).

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
| `my_accounts_tokens` | Token metadata (id, account_id, label, created_by, created_at; **never** secret) | Caller Admin+ on account | **done** (§7.10) |
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
| `create_account_token` (procedure) / `revoke_account_tokens` | Admin+ manage tokens; create → `Result<String, String>` (no panic) | **done** (§7.10 / `api.rs`) |
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
| Transfer recipient search | public catalog | `account_directory` | HTTP `GET /accounts?term&ledgerid` **done**; view for STDB clients |
| Send transfer | `transfer`, balances | `my_transfers` | `create_transfer` **done** + webhook enqueue |
| Pending finalize | | | `finalize_transfer` **done** + webhook enqueue |
| Transfers list / realtime | | `my_transfers` subscribe | replaces NATS subjects |
| Third-party bots / apps | `app`, `account_member`, `app_ticket` | `my_accounts_members` | tickets + grant/revoke **done** (§7.9) |
| Account API tokens + JSON HTTP | `account_token` | `my_accounts_tokens` **done** | create procedure + revoke + HTTP ping **done** (§7.10) |
| Legacy edge JSON `/api` + JetStream `stla_` | — | — | Superseded by §7.10 + edge reverse-proxy (§8.5); **no** legacy path shape |
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
6. ~~**Account API tokens + module HTTP:**~~ **done** (`api.rs`: create procedure, revoke reducer, `my_accounts_tokens`, `GET /ping` + `GET /account/ping`) — §7.10.
7. **Admin:** `grant_admin` / `revoke_admin`, optional `admin_patch_balance` (`update_account_address` + `ledger_audit` already exist).
8. **Hardening:** invariant tests, second-identity view isolation, webhook retry/dead-letter, app smoke (see §7.9 testing notes); restore Credit↔Credit (`Liability`) arm if still missing in `identify_transfer_kind`.

#### Explicit non-goals / accepted differences

| Topic | Note |
|-------|------|
| Pending transfers | In module; not in production `CreateTransfer` — keep |
| Bitflag permissions | Role enum is enough; UI only uses Admin today |
| NATS permission events | Drop if views+subscribe cover UI |
| JetStream user sessions (`sid`) / BitJita | **Hard drop**; BitAuth only on Topcoat edge |
| `ADMIN_KEY` HTTP header | **Drop**; `User.is_admin` only |
| Go lite edge | Replaced by Topcoat (§8); not extended |
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

**Decision (2026-07-29):** Account API tokens for **JSON HTTP** programmatic access. Apps (§7.9) remain for **native STDB clients**. Tokens are **not** SpacetimeDB Identities — they authenticate HTTP handlers only.

**Code:** `spacetimedb/src/api.rs` + `AccountToken` in `tables.rs`; `spacetimedb` Cargo feature **`unstable`** (HTTP handlers beta). **Status: initial slice done.**

#### Table (`account_token`)

```text
account_token
  id           u64 PK auto_inc
  account_id   u64  index btree
  token        String unique   -- opaque secret, no stla_ prefix; plaintext (private table)
  label        String          -- UI label (may be empty)
  created_by   Identity        -- user or app that minted it
  created_at   Timestamp
```

Token grants access to that account’s HTTP surface (start: account ping; more routes later).

#### Token management

| Function | Kind | Authz | Behavior |
|----------|------|-------|----------|
| `create_account_token(account_id, label, entropy)` | **Procedure** → `Result<String, String>` | Admin+ (human or app) | `entropy` ≥ 16 bytes of **client OS-random** material. Private `StdRng` (not timestamp-only `StdbRng`). `Ok(secret)` / `Err(message)`. Procedure `Result` is a normal return value (not reducer-style abort); **no panic** for validation errors. |
| `revoke_account_tokens(account_id, token_ids: Vec<u64>)` | Reducer | Admin+ | Delete listed ids; each must belong to account. Empty vec = no-op. |

**RNG note:** STDB `StdbRng` is always `seed_from_u64(timestamp_micros)` with no inject-entropy API; docs call it not cryptographically secure. Token mint must not rely on it alone when `created_at` is visible.

View `my_accounts_tokens`: id, account_id, label, created_by, created_at for accounts where caller is Admin+; **never** the secret.

#### Module HTTP surface (STDB handlers)

Public URL shape (host):

```text
$STDB_URI/v1/database/$DATABASE/route<path>
```

| Route (module path) | Auth | Response |
|---------------------|------|----------|
| `GET /ping` | None | `200` `pong` |
| `GET /accounts` | None | Public search: `term` + `ledgerid` (+ optional `limit`); address/username substring |
| `GET /account/ping` | token | `200` `pong` |
| `GET /account` | token | Account JSON (kind, balance, counters, …) |
| `GET /account/transfers` | token | Transfer list; query `limit` (default 100, max 1000), `offset` |
| `POST /account/transfers` | token | Create (token account = sender); `Idempotency-Key`; body `receivingId`, `amount`, optional `memo`/`pending` |
| `PUT /account/transfer` | token | Finalize pending (`transferId`, `amount`; 0 = void). Singular path until STDB path params |

Domain mutations share `create_transfer_core` / `finalize_transfer_core` with reducers (`TransferActor::Identity` vs `TokenAccount`). See `docs/api/accounts.md`.

`HandlerContext` has no caller Identity for custom tokens — lookup `account_token` by secret via `with_tx`.

**Route constraints (STDB 2.7):** exact match only; path chars = lowercase ASCII, digits, `-_~/`. No path params yet.

Auth header: raw secret only (`Authorization: <secret>`). No `Bearer ` prefix, no `stla_` prefix.

#### Relation to apps / edge

| Path | Use when |
|------|----------|
| **Apps + STDB SDK** | Realtime subscribe, reducers, same authz as humans |
| **Account tokens + HTTP** | Simple JSON/HTTP integrations, scripts, no STDB client |
| **Edge reverse-proxy** | Public domain + path reshape over module HTTP; not a domain re-implementation |

#### Manual smoke

```bash
# after publish
curl -s "$STDB/v1/database/stelofinance/route/ping"
# create token via procedure create_account_token(account_id, label, entropy)
# with OS-random entropy from the client, then:
curl -s "$STDB/v1/database/stelofinance/route/account/ping" \
  -H "Authorization: <secret>"
```

---

## 8. Lite webserver design (Rust / Topcoat)

**Code location (target):** workspace crate root (`src/`, `Cargo.toml`) — already a Topcoat spike.  
**Framework:** [Topcoat](https://github.com/tokio-rs/topcoat) (`topcoat` + `topcoat-cli` in flake).  
**Rule:** no domain/financial logic on the edge. Module owns money and privacy.

### 8.1 Stack & dependencies

| Concern | Choice |
|---------|--------|
| HTTP / HTML | Topcoat (`view!`, `#[page]`, `#[component]`, `#[layout]`) |
| Routing | **Module auto-discovery** (`Router::builder().discover()`) |
| Datastar | Topcoat `datastar` feature (SSE, `PatchElements`, `Signals`) |
| CSS | Topcoat `tailwind` feature + port of Stelo theme tokens |
| Assets | Topcoat `asset!` pipeline (content-hashed URLs) |
| Fonts | Topcoat **font helpers** (Source Code Pro) |
| Icons / illustrations | Topcoat `icon` / `IconData` (or equivalent) — SVG files, not hardcoded in page logic |
| STDB | Official **Rust** SpacetimeDB client + generated module bindings |
| OIDC | Rust OIDC client (library chosen at implement time; same BitAuth flow) |
| Deploy | Stateless Fly container; GHA for module publish + edge deploy |
| Drop | Go chi/tmpl, digitalxero, SQLite, NATS/JetStream, PostHog, BitJita, edge `ADMIN_KEY` |

**Env (edge):**  
`PORT`, `ENV`, `BITAUTH_ISSUER`, `BITAUTH_CLIENT_ID`, `BITAUTH_CLIENT_SECRET`, `BITAUTH_REDIRECT_URL`, `BITAUTH_LOGOUT_REDIRECT_URL`, `BITAUTH_OFFLINE_ACCESS`, `BITAUTH_SECURE_COOKIES`, `STDB_HOST`, `STDB_DATABASE` (and any Topcoat/cookie secrets if required).

### 8.2 Auth & cookies

1. Login page: **“Login with BitAuth”** → `GET /auth/bitauth/login` (PKCE S256, scopes `openid profile` [+ `offline_access`]).
2. `GET /auth/bitauth/callback` → code exchange; verify ID token (iss/aud/nonce).
3. Set cookies: `stdb_id_token`, optional `stdb_refresh_token`.
4. **Auto-refresh** when ID token is near expiry (use refresh cookie if present); rewrite `stdb_id_token` Max-Age from new `exp`. Do **not** store short-lived STDB websocket tokens over the OIDC ID token.
5. Requests to `/app/*`: require valid cookie (refresh if needed) → acquire pooled STDB connection as that identity.
6. Logout: clear cookies; optional BitAuth `end_session`.
7. Open-redirect protection on `?redirect=` (relative path only; same rules as Go `isValidRedirectURL`).

### 8.3 Request handling patterns

**Page load (HTML):**

```text
cookie → (refresh if near exp) → pool.acquire(identity)
  → one-off query / short subscribe of needed views
  → Topcoat view! render
  → respond HTML
```

Prefer **few view queries** that return render-ready shapes.

**Datastar live updates:**

```text
SSE open → STDB Subscribe (my_accounts / my_transfers / …)
  → on insert/update/delete → re-render fragment → PatchElements
  → on disconnect → unsubscribe / return conn to pool
```

Partial / component-specific patches (e.g. recipient fieldset) are **case-by-case** as each surface is ported.

**Errors (D30):** map STDB / OIDC / validation failures to:

- **Toasts** or inline Datastar patches for recoverable action failures;
- **Full error pages** for auth failures, hard missing resources, or unexpected 5xx;
- JSON error bodies for reverse-proxied API traffic.

**Module HTTP reverse-proxy:** see §8.5 — forward body/headers (including `Authorization`); reshape paths as needed; no domain math on the edge.

### 8.4 Types & codegen

1. Module schema is source of truth (`spacetimedb/`).
2. `spacetime generate` → Rust client bindings for the edge crate (regenerate in CI / Taskfile when module changes).
3. Edge uses typed reducers/views where available; SQL against **views** only (`SELECT * FROM my_accounts`), never private tables as a client.

### 8.5 Module HTTP reverse-proxy

**Goal:** expose programmatic JSON API on **our domain**, not only the SpacetimeDB host path `/v1/database/:db/route/...`.

| Property | Decision |
|----------|----------|
| Style | HTTP **reverse-proxy** (or thin path-mapping proxy) to module handlers |
| Auth | Edge **forwards** `Authorization` (account token secret); module validates |
| Path shape | **New** public routes (not legacy Go `/api/...`). Edge may introduce path params and map them onto module routes that lack param support today |
| Domain logic | None on edge — only route rewrite, header forward, status/body pass-through (plus optional error envelope consistency) |
| Docs | Update `docs/api/*` when public path shape is finalized |

Exact public path prefix (e.g. `/v1/...` vs something else) is chosen when implementing the first proxied routes; document then.

### 8.6 What gets deleted after cutover

- Entire Go edge: `cmd/app`, `web/`, `internal/*` (except anything still used by temporary scripts), Go `fly` binary path.
- `database/queries/*`, `database/gensql/*`, goose app migrations.
- Embedded NATS/JetStream; SQLite volume on Fly.
- digitalxero dependency; PostHog script; BitJita login; JetStream sessions/tokens.
- Go toolchain from flake/CI once no scripts require it (`scripts/seed-hexcoin` may move to Rust or keep `go run` temporarily).

### 8.7 Edge migration inventory (systems to recreate)

Work through these **one by one**. Status: `todo` until implemented in Topcoat. Source of “what exists” is the current Go edge; target is Topcoat unless marked drop.

#### A — Process / server foundation

| ID | System | Notes | Status |
|----|--------|-------|--------|
| A1 | HTTP process bootstrap | Port, graceful shutdown; no SQLite/NATS startup | todo |
| A2 | Router | **Topcoat module auto-discovery** | todo |
| A3 | Request logging | stdout / structured logs (D31) | todo |
| A4 | Panic / error recovery | Framework defaults + error pages | todo |
| A5 | CORS | If public API proxy needs browser/cross-origin; otherwise minimal | todo |
| A6 | Health check | Cheap path for Fly (replace `/api/ping` / `/heartbeat`) | todo |
| A7 | Response compression | gzip/brotli if easy in stack | todo |
| A8 | Env / config | BitAuth + STDB + PORT/ENV; drop JS_DIR/DB_FILE | todo |
| A9 | App-wide shared state | Topcoat app context: pool, OIDC client, config | todo |

#### B — Auth

| ID | System | Notes | Status |
|----|--------|-------|--------|
| B1 | BitAuth OIDC client | Discovery, PKCE, verify ID token, end_session | todo |
| B2 | Login / callback / logout routes | Port BitAuth flow | todo |
| B3 | Cookie jar | `stdb_id_token`, `stdb_refresh_token`, oauth state/nonce/pkce/redirect | todo |
| B4 | Open-redirect protection | Relative path only | todo |
| B5 | Authed `/app` gate | Cookie required; no JetStream `sid` | todo |
| B6 | Token auto-refresh near expiry | Best effort before STDB connect (D29) | todo |
| B7 | ~~BitJita login~~ | **Drop** | n/a |
| B8 | ~~JetStream user sessions~~ | **Drop** | n/a |
| B9 | ~~Edge `ADMIN_KEY`~~ | **Drop** — `User.is_admin` only | n/a |

#### C — SpacetimeDB client

| ID | System | Notes | Status |
|----|--------|-------|--------|
| C1 | Official Rust STDB SDK + bindings | `spacetime generate` into edge crate | todo |
| C2 | Connect-as-user | Cookie ID token → WithToken | todo |
| C3 | Per-Identity connection pool | **First milestone after C2** (§5.3) | todo |
| C4 | Page-load queries / reducers | Views + CallReducer via pool | todo |
| C5 | Live subscribe for Datastar | my_accounts / my_transfers etc. | todo |
| C6 | Error mapping | Toasts vs full pages vs JSON (D30) | todo |

#### D — Templates / UI structure

| ID | System | Notes | Status |
|----|--------|-------|--------|
| D1 | HTML shell layout | Public vs app chrome (Topcoat `#[layout]`) | todo |
| D2 | Marketing page `GET /` | Port index content | todo |
| D3 | Login page | **“Login with BitAuth”** button only (no BitJita UI) | todo |
| D4 | App pages | home, accounts, account detail, transfers, payment request | todo |
| D5 | Chrome components | nav, footer, app-nav, app-menu | todo |
| D6 | Partial / component patches | Case-by-case (e.g. transfer recipient) | todo |
| D7 | Display formatting | Asset-scale balances, relative times | todo |
| D8 | Idempotency keys in forms | Generate on render (uuid) | todo |

#### E — Datastar

| ID | System | Notes | Status |
|----|--------|-------|--------|
| E1 | Datastar JS asset | Topcoat `datastar` feature + asset pipeline | todo |
| E2 | SSE PatchElements | Accounts/transfers live updates + form actions | todo |
| E3 | Signals I/O | Topcoat `Signals` extractor | todo |
| E4 | Form posts | `@post` / form content type parity | todo |
| E5 | Hot reload | **Topcoat dev CLI** (`topcoat dev`) — no custom `/hotreload` | n/a (framework) |
| E6 | SSE reconnect / Last-Event-Id | Port where needed | todo |

#### F — CSS / fonts / static assets

| ID | System | Notes | Status |
|----|--------|-------|--------|
| F1 | Tailwind build | Topcoat `tailwind` feature | todo |
| F2 | Custom theme | melrose/anakiwa, source-code-pro, header-offset, etc. | todo |
| F3 | CSS delivery | **v1:** full site Tailwind as **inline `<style>`** if practical; else hashed `<link>` is acceptable. **Later ideal:** detect whether client has global TW cached → if not, inline (no layout shift) + lazy prefetch/cache the full asset; subsequent loads use `<link>` only. **Stretch:** per-page TW bundles for first paint, then graduate to global cache. | todo (v1) |
| F4 | Fonts | Topcoat **font helpers** (Source Code Pro variable) | todo |
| F5 | Favicon | Via asset pipeline | todo |
| F6 | Static serving | Topcoat assets + cache headers | todo |
| F7 | Content-hash cache busting | Topcoat assets (replaces Go `hash_asset_path`) | todo |

#### G — Icons & illustrations

| ID | System | Notes | Status |
|----|--------|-------|--------|
| G1 | SVG icons | Sized/colored via class/`currentColor` without hardcoding SVG into Rust page code | todo |
| G2 | Illustrations | Same pattern | todo |
| G3 | Asset inventory | Icons: logo-full, logo-colored, home, wallet, transfer, bitcraft, discord, github, close, hamburger, market-candles, right-arrow. Illustrations: account, deposit, nintron, trade, withdraw. | todo |

#### H — App HTML surfaces (call module; no domain rules)

| ID | Surface | Go routes (reference) | Status |
|----|---------|----------------------|--------|
| H1 | Accounts list + create + live updates | `GET/POST /app/accounts`, `GET .../updates` | todo |
| H2 | Account admin | detail, primary, users, tokens | todo |
| H3 | Transfers UI | list, select, recipient search, submit | todo |
| H4 | Payment request | `GET /app/request`, `POST .../transfers` | todo |
| H5 | Logout | clear cookies / end_session | todo |

#### I — API reverse-proxy

| ID | System | Notes | Status |
|----|--------|-------|--------|
| I1 | Reverse-proxy to module HTTP | Our domain; path reshape; not legacy `/api` shape | todo |
| I2 | Auth header forward | Edge does not re-validate account tokens | todo |
| I3 | Route map + docs | Map each module handler to public edge path; update `docs/api/*` | todo |

#### J — Observability / product extras

| ID | System | Notes | Status |
|----|--------|-------|--------|
| J1 | Logging | stdout (structured if easy) | todo |
| J2 | Log level | Env / compile-time; no NATS `logs.level` | todo |
| J3 | ~~PostHog~~ | **Drop** | n/a |

#### K — Dev experience & deploy

| ID | System | Notes | Status |
|----|--------|-------|--------|
| K1 | Dev server | **`topcoat dev`** for edge; later Taskfile recipe that runs STDB + Topcoat together | todo |
| K2 | Taskfile | Keep `stdb:*`; replace Go `build`/`live` with Cargo/Topcoat; add combined dev task | todo |
| K3 | Nix flake | Keep Rust/wasm/spacetime/topcoat-cli; drop Go toolchain when edge + scripts no longer need it | todo |
| K4 | Fly | Stateless `fly.toml` (no volume); health check; secrets | todo |
| K5 | CI | GHA: publish module to STDB + build/deploy edge container | todo |
| K6 | Binary packaging | `cargo build --release` edge binary in image | todo |

#### L — Explicit non-edge (module or delete)

| System | Disposition |
|--------|-------------|
| SQLite / sqlc / goose | Delete after cutover |
| NATS / JetStream | Delete after cutover |
| `internal/accounts/*` domain | Already → module reducers |
| Webhook worker | Module `deliver_webhook` |
| Go digitalxero client | Replaced by Rust SDK |
| JetStream account tokens | Module `account_token` |

### 8.8 Suggested edge implementation order

1. Skeleton: Topcoat app, layout, theme, fonts, favicon, health (A, F, G baseline).
2. Datastar script + asset pipeline (E1).
3. BitAuth + cookies + login page (B, D3).
4. STDB connect-as-user (C1–C2).
5. **Identity connection pool** (C3).
6. One app page from STDB + Datastar subscribe (C4–C5, H1 slice).
7. Error mapping polish (C6).
8. Remaining app surfaces (H2–H5, D6 case-by-case).
9. Module HTTP reverse-proxy (I1–I3).
10. Fly + GHA cutover; delete Go edge (K4–K6, §8.6).

---

## 9. API compatibility

### 9.1 External JSON API

**Target:** module HTTP handlers (§7.10) are the domain implementation. The **Topcoat edge reverse-proxies** them onto **our domain** with a **new public path shape** (not legacy Go `/api/...`). Edge may reshape routes (e.g. path params) when STDB module HTTP cannot express them yet (§8.5).

| Area | Strategy |
|------|----------|
| Ping (public + account) | Module HTTP done; proxy first |
| Account get / search / transfers / finalize | Module HTTP (partial done); proxy + expand as needed |
| Ledgers / audit / admin | Views + admin reducers; HTTP + proxy when needed |
| Webhooks get/put/delete | Views + reducers / HTTP + proxy when needed |

**Breaking change vs Go `/api`:** intentional. Update `docs/api/*` when edge public paths land. STDB IDs may differ from old SQLite integers (u64 vs int64) — document at cutover.

### 9.2 Direct STDB clients (apps)

Partners use official SDKs (TS/Rust/C#) with **SpacetimeAuth anonymous** tokens (after `create_app_ticket` + first connect bind), and an account Admin+ `grant_account_member` with the app Identity. Same reducers/views as humans (`effective_role`). Edge is optional for them. See §7.9.

### 9.3 Account HTTP tokens

Partners that want plain HTTP/JSON (no STDB SDK) use **account API tokens** + module routes, typically reached via the **edge reverse-proxy** so the hostname is Stelo’s. Edge forwards `Authorization`; does not reimplement token validation. Orthogonal to apps: same account may have both members and HTTP tokens.

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
| **P0 — Module spike** | Module skeleton + BitAuth connect + tables; transfers, views, ACL, webhooks, apps, tokens | Domain core usable via STDB (largely **done**) |
| **P1 — Topcoat edge foundation** | Topcoat skeleton, theme/assets, BitAuth, STDB connect, **Identity pool**, one STDB-backed page + Datastar | Browser login + one live page without Go domain path |
| **P2 — App surface parity** | Port remaining `/app` pages/actions (inventory §8.7 H*) | Full web UX on Topcoat + STDB |
| **P3 — API reverse-proxy** | Edge proxy of module HTTP; new public paths; docs | Third parties hit our domain; no Go `/api` |
| **P4 — Domain leftovers** | Admin reducers, any missing HTTP routes | Parity matrix green |
| **P5 — Import** | Script/reducer import of users/accounts/balances/transfers | Audit invariant holds; tester accounts usable |
| **P6 — Cutover** | Mainnet module; Topcoat on Fly (stateless); GHA deploys; decommission Go + SQLite + NATS volumes | Stable prod; old DB read-only archive |
| **P7 — Cleanup** | Delete Go edge/toolchain; flake/Taskfile/docs hygiene | Rust-only product path |

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
| Q2 | Cookie details: name, Max-Age, rotation, logout | **Mostly decided** | `stdb_id_token` / `stdb_refresh_token`; **auto-refresh near expiry** (D29); exact refresh skew TBD |
| Q3 | OIDC claim mapping | **Decided (dev)** | Stable `sub` = player id; `preferred_username` = display; see §7.2 |
| Q4 | Account API token validation path | **Decided** | Module validates; edge reverse-proxies + forwards `Authorization` |
| Q5 | Exact table/view/reducer names | **In flux** | Live schema in `spacetimedb/src/` |
| Q6 | Pending transfer flags / states | **Decided** | `TransferState` + pending/posted; webhooks `kind`/`state` strings (§7.5) |
| Q7 | ~~digitalxero drift~~ | **Superseded** | Edge moves to official Rust SDK (D10) |
| Q8 | Admin auth | **Decided** | `User.is_admin` only; no edge `ADMIN_KEY` |
| Q9 | Backups / PITR / disaster recovery on mainnet | Open | Ops runbook |
| Q10 | Connection pooling by identity | **Decided** | Per-Identity pool = first milestone after connect (D23, §5.3) |
| Q11 | Whether import recomputes balances from transfers vs copies balances | Open | Recompute is safer if history complete |
| Q12 | Public edge API path prefix / shape | Open | Chosen when implementing I1; not legacy `/api` |
| Q13 | CSS delivery: pure inline vs link vs cache-aware hybrid | **v1 = inline if practical** | Stretch goals in §8.7 F3 |
| Q14 | Rust OIDC library choice | Open | Same BitAuth flow as Go spike |

---

## 14. Testing strategy

| Layer | What |
|-------|------|
| Module unit/integration | Reducer tests: valid transfer, insufficient funds, idempotent replay, idempotent conflict, cross-ledger reject, permission deny |
| Invariant tests | Random transfer sequences → ledger sums to 0 |
| View tests | User A cannot read user B balances via views |
| Edge smoke | Topcoat: OIDC login, HTML render, Identity pool, Datastar SSE update on transfer, proxy ping |
| Webhook tests | Outbox retry; HTTP mock; no open redirects |
| Import dry-run | Snapshot of prod export against local STDB |

---

## 15. Risks and mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Wrong authz → fund theft / data leak | Critical | Private tables; view filters; reducer checks; adversarial tests |
| Topcoat / early framework churn | Medium | Pin Topcoat version; thin edge; upstream issues as needed |
| Official Rust STDB client gaps | Medium | Pin SDK; thin wrapper; monitor STDB releases |
| Token-in-cookie theft (XSS) | High | HttpOnly Secure cookies; tight CSP; no token in JS |
| Webhook SSRF from module HTTP | High | URL allowlist/block private ranges; no redirects; timeouts |
| Mainnet ops unfamiliarity | Medium | Local parity; runbooks; staged publish |
| New API path shape breaks Go-era clients | Medium | Document break; no legacy `/api` promise |
| Per-request connect latency | Medium | **Mitigated by Identity pool (D23)** early |

---

## 16. Success metrics

1. SQLite and NATS **not required** in production; Go edge **gone**.
2. All transfers and balance changes happen only in module reducers.
3. Browser session uses STDB user identity (cookie token); views enforce isolation; edge uses official Rust client + Identity pool.
4. Programmatic JSON API available on **Stelo domain** via reverse-proxy (new paths documented).
5. Webhooks deliver with durable retries without JetStream.
6. Ledger audit invariant holds post-import and under test suites.
7. A second client (e.g. small TS script) can call `create_transfer` / subscribe to `my_transfers` with a user token and see the same authz behavior as the website.

---

## 17. Suggested repo layout (post-refactor)

```text
stelofinance/
  spacetimedb/            # Rust SpacetimeDB module (CLI default path)
    src/lib.rs            # init, client_connected, OidcProvider, …
    src/tables.rs         # domain schema
    src/views.rs          # multi-tenant + public views
    src/acl.rs            # grant/revoke, primary, webhook
    src/apps.rs           # app_ticket + connect-time bind
    src/api.rs            # account_token + module HTTP
    src/transfers.rs      # create_transfer, finalize_transfer
    src/webhooks.rs       # webhook_delivery + deliver_webhook
    Cargo.toml
  src/                    # Topcoat lite edge (workspace package)
    main.rs / app/…       # module-discovered routes, layouts, pages
    module_bindings/      # spacetime generate output (or equiv path)
  Cargo.toml              # edge package: topcoat + spacetimedb client
  spacetime.json
  spacetime.dev.json
  # spacetime.local.json  # gitignored personal overrides
  scripts/                # seed/diag (Rust preferred long-term)
  Taskfile.yml            # stdb:* + edge dev (topcoat) + combined live
  fly.toml                # stateless edge (no SQLite/NATS volume)
  .github/workflows/      # publish module + deploy Fly container
  tmp/spacetimedb/
  docs/design/spacetimedb-refactor.md
  docs/api/               # public API contract (edge paths + payloads)
```

**Local workflow (target):** `task stdb:start` + module watch; `topcoat dev` (or Taskfile wrapper) for edge. Later: one Taskfile command for both. Wipe `tmp/spacetimedb` if snapshot/identity errors appear.

**During transition:** Go tree (`cmd/app`, `web/`, `internal/`) may still run production until P6 cutover; do not add new Go features.

---

## 18. Spike checklist

### 18.1 Module / domain (P0 — largely done)

- [x] Module crate + `spacetime.json` / `.dev.json` + Taskfile `stdb:*`
- [x] Private domain tables (+ public `ledger`); enums; idempotency table + index
- [x] `config.owner` at init; owner connect for CLI SQL
- [x] `client_connected`: BitAuth iss/aud + User upsert (`Identity` PK, `preferred_username`)
- [x] `User.is_admin` + `require_admin` helper (admin reducers TBD)
- [x] BitAuth OIDC on **Go** parallel routes + cookies (`stdb_id_token`) — historical spike
- [x] Go STDB connect smoke (`/auth/bitauth/stdb-connect`) — historical
- [x] Document OIDC claims + cookie names (§5.2 / §7.2)
- [x] `create_ledger` + `create_account` + transfers + seed + views + ACL + webhooks + apps + account tokens/HTTP
- [ ] Prove a second identity cannot read the first identity’s view data
- [ ] Admin reducers (`grant_admin` / `revoke_admin`, optional `admin_patch_balance`)

### 18.2 Topcoat edge (P1 — active track)

Track status in §8.7 inventory. Minimum P1 exit:

- [ ] Topcoat skeleton + auto-discovery routes + layout + theme/fonts/favicon
- [ ] BitAuth OIDC on Topcoat + login page (“Login with BitAuth”)
- [ ] Official Rust STDB client + generated bindings; connect-as-user
- [ ] Per-Identity connection pool
- [ ] One app page from STDB + Datastar subscribe → patch on transfer
- [ ] Health check for deploy; stdout logging
- [ ] (Follow-on) reverse-proxy slice for module `ping` routes

---

## 19. Appendix A — Current → target mapping

| Current component | Target |
|-------------------|--------|
| Go edge (`cmd/app`, `web/`, `internal/*`) | **Topcoat** edge crate (`src/`) |
| Go `tmpl` + Datastar | Topcoat `view!` + `datastar` feature |
| `gensql` models | Module tables + **Rust** client bindings |
| `accounts.CreateTransfer` | `create_transfer` reducer |
| `accounts.EventTransfer` + NATS publish | STDB row updates + webhook schedule |
| JetStream sessions KV | `stdb_id_token` cookie (+ module `User`) |
| JetStream account tokens | **`account_token` + module HTTP** + edge reverse-proxy |
| JetStream webhook stream | `webhook_delivery` + `deliver_webhook` |
| `AppAccountsUpdates` NATS subs | STDB subscribe on views → Datastar |
| `ADMIN_KEY` middleware | **Dropped** → `User.is_admin` |
| Goose / sqlc | `spacetime publish` + views/reducers |
| BitJita login KV | **Dropped** → BitAuth only |
| Go `/api/*` | **New** edge proxy paths (not same shape) |
| digitalxero Go client | Official Rust STDB SDK |
| Fly volume (SQLite/NATS) | **Stateless** edge; module on STDB cloud |

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

1. ~~Module domain core (tables, views, transfers, ACL, webhooks, apps, tokens/HTTP)~~ **done** (§7).
2. ~~Document Topcoat edge decision + full migration inventory~~ **done** (§5, §6 D2/D26–D34, §8).
3. **P1 — Topcoat foundation:** skeleton, layout, Tailwind/fonts/icons, BitAuth, STDB connect (follow §8.8 order).
4. **Identity connection pool** immediately after connect works.
5. One STDB-backed app page + Datastar live updates.
6. Remaining app surfaces (§8.7 H*); case-by-case partials (D6).
7. Module HTTP reverse-proxy (§8.5 / I*); update `docs/api/*`.
8. Admin reducers on module (parallel track).
9. Fly stateless + GHA (module publish + edge deploy); cut over; delete Go.

Work items: tick §8.7 inventory and §18.2 as they land.

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
| 2026-07-29 | **§7.10 implemented:** `AccountToken` (+label, created_by); `create_account_token` procedure returns secret; `revoke_account_tokens(Vec)`; `my_accounts_tokens`; HTTP `GET /ping` + `GET /account/ping`; `unstable` feature |
| 2026-07-30 | Token mint: client `entropy` arg + private `StdRng` (avoid timestamp-only `StdbRng`); auth header raw secret only; view accessor `my_accounts_tokens` |
| 2026-08-01 | `create_account_token` returns `Result<String, String>` instead of panicking on validation errors (avoids fatal WASM procedure errors) |
| 2026-08-01 | HTTP account API: `GET /account`, list/create transfers, `PUT /account/transfer` finalize; shared transfer cores + `TransferActor` |
| 2026-08-01 | Public HTTP account search: `GET /accounts?term&ledgerid` (limit default 10 max 50) |
| 2026-08-02 | **Edge language change:** replace Go lite server with **Rust Topcoat**; official STDB Rust client; BitJita / `ADMIN_KEY` / PostHog / legacy `/api` shape **dropped**; Identity pool first milestone after connect; reverse-proxy for module HTTP on our domain; full edge inventory §8.7; phases/repo layout/decisions updated |

---

## SOURCE OF TRUTH

This document should remain the source of truth during this refactor. If the user presents things contrary to this document or in addition to this document, inform the user of that deviation/addition and **update this document** to reflect their decision. Live schema details in `spacetimedb/src/` win on drift until synced here.
