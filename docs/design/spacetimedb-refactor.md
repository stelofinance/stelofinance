# Design Doc: SpacetimeDB Refactor

**Status:** Outline / decisions captured  
**Date:** 2026-07-24  
**Author:** Stelo maintainers + design discussion  
**Related:** Current stack is Go + SQLite (sqlc/goose) + embedded NATS/JetStream + Datastar

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

**Decision:** Prefer **BitCraft OIDC** over BitJita-style login calls. Store a **SpacetimeDB client token** in an HTTP-only cookie. The lite webserver uses that cookie on each request to open (or later pool) an STDB connection **as that identity**.

Desired properties:

- Edge is **not** a god-mode service identity for user reads/writes.
- Module authorization is based on `ctx.sender()` (STDB identity), so later direct clients reuse the same reducers/views.
- Page render path: **one STDB session / identity context** loads the data needed for the template (via one-off queries or short-lived subscribe), rather than many ad-hoc SQLite round-trips scattered through handlers. Goal: avoid “N independent DB calls per render” as a pattern; batch via views where possible.

**Open implementation details (see §13):**

- Exact cookie names, Secure/SameSite, TTL vs STDB token lifetime.
- Whether STDB token is the long-lived server-issued token vs short-lived websocket token (must not overwrite long-lived with short-lived).
- First-time link: OIDC subject → `user` row (`client_connected` / `link_bitcraft_user` reducer).
- Account API tokens for `/api/accounts/{id}/*` remain a separate mechanism (capability tokens), not the browser cookie.

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

---

## 7. Module design (Rust)

### 7.1 Table inventory (proposal — names open to revision)

All core tables **private** unless noted.

| Table | Purpose | Notes |
|-------|---------|--------|
| `user` | Stelo user profile | Keyed by auto id; unique on BitCraft id / OIDC subject / STDB `Identity` |
| `ledger` | Asset type / scale / code | Admin-managed |
| `account` | Wallet / balance container | Balances: debits/credits pending/posted; address; code; flags; optional `user_id`; webhook URL |
| `account_permission` | User ↔ account ACL | Bitflags (`PermAdmin`, `PermReadBal`, …) |
| `transfer` | Immutable-ish transfer records | Debit/credit accounts, amount, ledger, code, flags, memo, timestamps |
| `transfer_idempotency` | `(account_id, key) → transfer_id + request_hash` | Preserve conflict vs replay semantics |
| `account_token` | Hashed API tokens for account-scoped HTTP API | Never store raw token; store hash + metadata |
| `webhook_outbox` | Durable delivery jobs | Schedule column → procedure; status, attempts, next_run |
| *(optional)* `admin_audit` | Admin mutations log | If needed beyond transfer history |

**Public surface:** prefer **views**, not public base tables, for anything with balances or PII.

Naming note: final snake_case table/reducer/view names are TBD; the above mirrors current domain language.

**SpacetimeDB constraint limits (module `spacetimedb/src/tables.rs`):** STDB 2.7 does not support multi-column unique or composite primary keys. SQLite composites are mapped as:

| SQLite constraint | Module approach |
|-------------------|-----------------|
| `UNIQUE (address, ledger_id)` on `account` | Synthetic unique `address_ledger_key` = `"{ledger_id}\0{address}"` |
| `UNIQUE (user_id, ledger_id)` on `account` | Multi-column btree `by_user_ledger`; uniqueness enforced in reducers when added (nullable `user_id` complicates a synthetic unique) |
| `PRIMARY KEY (account_id, key)` on `transfer_idempotency` | Auto-inc PK + synthetic unique `account_key` = `"{account_id}\0{key}"` |
| Logical one-row-per `(account_id, user_id)` on `account_permission` | Synthetic unique `account_user_key` = `"{account_id}\0{user_id}"` |

Helpers: `address_ledger_key`, `account_user_key`, `idempotency_account_key` in the same file. No DB-level foreign keys; referential integrity is reducer-enforced later.

Also on `user`: unique `identity` (`Identity`) for `ctx.sender()` linkage (in addition to BitCraft fields).

### 7.2 Identity & user linking

```text
BitCraft OIDC login (browser)
    → edge completes OIDC
    → STDB connection with OIDC JWT (or exchange to STDB token)
    → module client_connected / ensure_user:
         - validate issuer + audience
         - upsert user row linked to Identity + bitcraft claims
    → edge stores STDB access token in HttpOnly cookie
```

Reducers must treat `ctx.sender()` as the principal. Map:

`Identity` → `user` → permissions on `account`.

Admin operations: OIDC claims and/or a dedicated admin identity list / role claim — **not** a long-term shared `ADMIN_KEY` env for module logic (edge may keep a break-glass path during migration only).

### 7.3 Views (multi-tenant, caller-dependent)

Views use `ViewContext` and filter by `ctx.sender()`. Prefer indexed lookups; use query-builder views where joins/filters are declarative.

| View (proposal) | Returns | Auth idea |
|-----------------|---------|-----------|
| `my_user` | Current user profile | Caller’s row only |
| `my_accounts` | Accounts caller can see | Via `account_permission` (and primary ownership) |
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
| `ensure_user` / lifecycle connect | Link identity ↔ user |
| `create_account` | Create account + owner permission |
| `update_account_address` | Admin/system |
| `set_account_user` | Link/unlink primary user |
| `grant_permission` / `revoke_permission` | Account ACL |
| `create_transfer` | Full transfer path + idempotency + enqueue webhook outbox |
| `set_webhook` / `clear_webhook` | Account webhook URL |
| `create_account_token` / `revoke_account_token` | API tokens (return raw token **once** to caller) |
| `create_ledger` | Admin |
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

**Browser flow (target):**

1. User hits login → redirect to **BitCraft OIDC**.
2. Callback on lite server validates OIDC tokens.
3. Edge connects to STDB with OIDC JWT (or performs identity/token handshake per STDB docs).
4. Module ensures `user` exists for that identity.
5. Edge sets **HttpOnly Secure cookie** with STDB access token (long-lived identity token; do not confuse with short-lived websocket tokens).
6. Subsequent requests: read cookie → STDB client `WithToken` → act as user.

**Account token API (third party):**

- Header `Authorization: <token>` as today.
- Edge hashes token, looks up via one-off query or dedicated reducer/view, **or** better: call reducers that accept the capability only after module validates the hash.
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
| **P0 — Spike** | Rust module skeleton: user, account, transfer, one view, create_transfer; Go edge connect + cookie token + one Datastar page | End-to-end transfer visible in UI via STDB |
| **P1 — Auth** | BitCraft OIDC + STDB token cookie; ensure_user; issuer/aud checks | Login works without BitJita/JetStream login KV |
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
| Q1 | Full threat model (abuse, spam transfers, energy, anonymous connect policy) | **Follow up later** | Module is public; need issuer allowlists, rate limits, connect policy |
| Q2 | Cookie details: name, Max-Age, rotation, logout (token revoke?) | Open | Align with STDB token lifecycle docs |
| Q3 | OIDC claim mapping (BitCraft subject → bitcraft_id / username) | Open | Inspect real BitCraft OIDC claims |
| Q4 | Account API token validation path (edge hash lookup vs pure reducer) | Open | Prefer module-side verification |
| Q5 | Exact table/view/reducer names | Open | Owner may revise naming |
| Q6 | Pending transfer flags (schema supports; logic incomplete today) | Open | Implement or explicitly defer |
| Q7 | digitalxero vs STDB protocol drift process | Monitor | Pin versions; CI smoke test |
| Q8 | Admin auth long-term (claims vs allowlist identities) | Open | Replace env `ADMIN_KEY` for module ops |
| Q9 | Backups / PITR / disaster recovery on mainnet | Open | Ops runbook |
| Q10 | Connection pooling by identity | Deferred | v1 per-request OK |
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
    src/
    Cargo.toml
  spacetime.json          # project defaults (database, module-path)
  spacetime.dev.json      # shared dev env (server: local)
  # existing Go module becomes the lite edge
  cmd/app/
  internal/handlers/      # thin adapters only
  internal/stdb/          # connection helpers, generated bindings
  web/templates/
  docs/
    api/                  # external HTTP docs (façade)
    design/
      spacetimedb-refactor.md
  scripts/
    import_from_sqlite.py # or .go / .rs
```

---

## 18. Spike checklist (P0)

Use this to validate the design before full port:

- [x] Publish local Rust module with private `user` / `ledger` / `account` / `account_permission` / `transfer` / `transfer_idempotency` (indexes + unique constraints; seed deferred)
- [ ] `create_transfer` reducer + idempotency
- [ ] `my_accounts` / `my_transfers` views filtered by sender
- [ ] Go edge: connect with token, one-off query views, render one app page
- [ ] Go edge: SSE + subscribe → Datastar patch on transfer
- [ ] Prove a second identity cannot read the first identity’s view data
- [ ] Document BitCraft OIDC claim fields actually received
- [ ] Decision memo: cookie name + token persistence rules

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
| `ADMIN_KEY` middleware | Temporary edge break-glass → module admin claims |
| Goose migrations | `spacetime publish` module versioning |
| sqlc | Module query builder / views |

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

1. Review this outline; freeze **D1–D11** or amend.
2. Resolve **Q2–Q5** enough to start P0 spike.
3. Create `spacetimedb/` Rust crate and wire local `spacetime dev`.
4. Spike OIDC with BitCraft (claims dump) in a branch.
5. After P0, expand this doc from **outline** to **implementation spec** (exact schemas, reducer signatures, error codes, cookie RFC).

## SOURCE OF TRUTH
This document should remain the source of truth during this refactor, as such, if the user presents things contrary to this document or in addition to this document, you should inform the user of that deviation/addition and update this document to reflect their decision.
