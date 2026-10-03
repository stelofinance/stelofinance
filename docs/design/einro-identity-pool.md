# Design Doc: einro — SpacetimeDB Connection Pool

**Status:** Token-keyed, minimal pool (`src/einro/`); name **`einro`** (placeholder for extract)  
**Date:** 2026-08-04 (updated 2026-08-05)  
**Related:** [spacetimedb-refactor.md](./spacetimedb-refactor.md) §5.3, D23, inventory C3  
**Goal:** Reuse live SpacetimeDB client connections with **minimal** logic. No JWT crypto, no Identity derivation, no subscription management.

---

## 1. Summary

**einro** is a map of **bearer token → live connection**.

```text
acquire(token):
  if pool has active conn for this exact token string → return it
  else connect(token) via Connector → store → return

on STDB/disconnect error → drop that slot
idle TTL with no holders → drop slot
```

**Does not:**

- Peek or verify JWTs (`iss`/`sub`/`exp`)
- Key by SpacetimeDB `Identity`
- Take an expiration argument
- Manage subscriptions (app: query-builder presets + own handles)
- Refresh OIDC tokens (app / BitAuth)

**Does:**

- Open / reuse / idle-evict / disconnect
- Let **SpacetimeDB** accept or reject the token at connect time
- Drop connections when the app observes they are dead (`!is_active` or connect/use errors)

---

## 2. Why this model

Earlier designs used Identity keys and/or JWT verify-before-reuse to avoid “piggyback” attacks and to share one socket per user. That added JWKS, crypto (~10ms+), and IdP coupling into the pool.

**Token equality as the only key** is simpler and still safe for pooling:

| Concern | How it’s handled |
|---------|------------------|
| Forged token attaches to victim’s socket | **No** — only the **exact** token string reuses a slot |
| Expired token | Caller stops sending it; new token → new slot. STDB may reject connect. Live sockets may outlive JWT `exp` until idle drop or STDB disconnect — **must measure (E1)** |
| Multi-device same user | Different tokens → different connections (fine at small N) |
| Token refresh | App acquires with the **new** string; old slot idles out |

**Tradeoffs (accepted):**

- After refresh, a brief window can have **two** connections for the same user (old + new) until idle TTL.
- Same user on two devices → two connections (expected, low cost).
- einro does **not** interpret expiry; the consumer must present a usable token.

---

## 3. Strong recommendation: test STDB expiry behavior

**E1 + STDB source (2026-08-07/08):** SpacetimeDB does **not** close a live client when the OIDC JWT used at connect has since expired (confirmed in host code + smoke). New connects accept JWTs only up to **~60s past `exp`** (host leeway). Edge session freshness is still app-owned: `ensure_bearer` refreshes when within **`TOKEN_REFRESH_SKEW_SECS` (15s)** of `exp` so acquires stay well inside that 60s window.

### Experiment E1

| Step | Action | Record |
|------|--------|--------|
| 1 | Connect with OIDC token; keep a pooled connection | ✓ (temporary smoke harness) |
| 2 | Hold past JWT `exp` (no reconnect) | **`is_active` stayed true; sub active / not ended** (>3 min past exp observed) |
| 3 | If STDB disconnects | **Did not disconnect** (in this run) |
| 4 | New connect with **expired** token | **Succeeded** (see probe note below) |
| 5 | Acquire with **fresh** token | Not required for kick behavior; still true that new string → new pool slot |

### Harness

Temporary Topcoat page `/stdb-smoke` + SSE monitor (removed after E1). Results retained in the log below.

### Experiment log

| When | STDB version | Result |
|------|--------------|--------|
| 2026-08-07 | local (dev; 2.7.x stack) | Live conn + sub survived **>3 min past JWT `exp`**. New full client connect with same expired token **succeeded** (within short post-exp window). No host kick. |
| 2026-08-08 | STDB source | New connect JWT leeway **~60s** past `exp`; live connections **not** closed when connect token later expires. Edge skew set to **15s**. |

---

## 4. Responsibility split

```text
┌─────────────────────────────────────────────────┐
│ App (Stelo)                                     │
│  BitAuth login / refresh / cookies              │
│  Present current bearer token to einro          │
│  Query-builder subscription presets             │
│  Handlers: subscribe / unsubscribe own handles  │
│  Browser SSE reconnect + token refresh (below)  │
│  On STDB errors: invalidate / retry with token  │
└──────────────────────┬──────────────────────────┘
                       │ token: &str
┌──────────────────────▼──────────────────────────┐
│ einro                                           │
│  Map token → conn; idle TTL; drop dead slots    │
└──────────────────────┬──────────────────────────┘
                       │ Connector::connect(uri, db, token)
┌──────────────────────▼──────────────────────────┐
│ Adapter + spacetimedb-sdk + module_bindings     │
└─────────────────────────────────────────────────┘
```

### 4.1 Browser SSE reconnect (app-owned; not einro)

E1 showed STDB will happily keep (and re-open) sockets with an expired JWT. Production concern is different:

1. **Browser ↔ edge SSE** drops (network blip, proxy idle timeout, tab sleep).
2. Datastar/EventSource **reconnects** the SSE to the edge.
3. Edge must open (or reuse) an STDB client for that request using a **usable** token — preferably refreshed.

Target flow for long-lived Datastar handlers:

```text
SSE open (or re-open)
  → read bitauth_token (+ refresh cookie)
  → if ID token missing / near exp / past exp:
        try BitAuth refresh → rewrite bitauth_token cookie
        if refresh fails → patch error / end SSE (client can send user to login)
  → pool.acquire(current_token_string)   // new string after refresh → new slot
  → subscribe (app presets); stream patches
  → on STDB !is_active or use error:
        pool.invalidate_token(old_token) if needed
        optional one retry: refresh again → acquire(new) → resubscribe
        else end SSE with error patch
  → on SSE end (client gone): unsubscribe; drop PooledConn (idle clock)
```

**einro stays dumb:** exact token string key only. Refresh, cookie rewrite, “should we retry?”, and login redirect UX all live in the app (B6 + SSE handlers).

---

## 5. Decisions

| ID | Choice |
|----|--------|
| **D1** Token refresh | **External** (app) |
| **D2** Pool key | **Exact bearer token string** (not Identity) |
| **D3** JWT peek / verify in pool | **None** — STDB validates at connect |
| **D4** Expiry argument | **None** |
| **D5** Live socket past JWT exp | **Survives** — STDB does not kick mid-connection; **new** connect: ~60s post-`exp` leeway only |
| **D6** Idle TTL | Yes — drop unused slots after TTL |
| **D7** Dead connection | Drop slot when `!is_active` or failed open |
| **D8** Subscriptions | App-owned |
| **D9** Name | einro |
| **D10** Message pump | Connector/adapter |
| **D11** Security model | **Token equality** (same string → same slot) |

---

## 6. API sketch

```rust
pub struct PoolConfig {
    pub max_connections: usize,
    pub idle_ttl: Duration,
    pub connect_timeout: Duration,
}

// IdentityPool::acquire(&self, token: &str) -> Result<PooledConn, PoolError>
// IdentityPool::invalidate_token(&self, token: &str)
// PooledConn: Deref to Conn; Drop returns interest (idle clock)
```

`Connector` still provides module-specific `DbConnection` construction. Optional: `Conn` may expose `identity()` after connect for app logging (not used as pool key).

---

## 7. Code layout

```text
src/einro/     # token-keyed pool only (no jsonwebtoken)
src/stdb/      # StdbConnector, StdbConfig, helpers (no JWKS validator)
```

---

## SOURCE OF TRUTH

Update this file when keying or expiry strategy changes. **STDB: no mid-connection kick on JWT exp; ~60s leeway on new connect. App: `ensure_bearer` skew 15s + reconnect.**
