use crate::require_account_role;
use crate::require_principal;
use crate::tables::*;
use spacetimedb::http::{Body, HandlerContext, Request, Response, Router, handler, router};
use spacetimedb::{
    ProcedureContext, ReducerContext, Table, TxContext, procedure,
    rand::{Rng, RngCore, SeedableRng, rngs::StdRng},
    reducer,
};

/// Alphabet for opaque token secrets (no ambiguous punctuation).
const TOKEN_CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
const TOKEN_LEN: usize = 32;
const TOKEN_GEN_ATTEMPTS: usize = 4;
const MIN_ENTROPY_LEN: usize = 16;

// ---------------------------------------------------------------------------
// Token lifecycle (Admin+ on account; humans or apps)
// ---------------------------------------------------------------------------

/// Create an HTTP API token.
///
/// `entropy` must be caller-generated secret material (min 16 bytes). SpacetimeDB's
/// built-in `ctx.rng()` is seeded only from the call timestamp, which is public
/// (`created_at`), so we seed our own `StdRng` from `entropy` + timestamp + sender.
///
/// Returns `Result<String, String>` as a normal procedure payload (Ok = secret,
/// Err = message). Unlike reducers, procedure `Err` is not a host-level failure —
/// it is returned to the caller. Do **not** `panic!` for validation errors (that
/// surfaces as a fatal WASM error in the host log).
#[procedure]
pub fn create_account_token(
    ctx: &mut ProcedureContext,
    account_id: u64,
    label: String,
    entropy: String,
) -> Result<String, String> {
    // try_with_tx may retry the closure, so clone inputs rather than move.
    ctx.try_with_tx(|tx| {
        create_account_token_tx(tx, account_id, label.clone(), entropy.clone())
    })
}

fn create_account_token_tx(
    tx: &TxContext,
    account_id: u64,
    label: String,
    entropy: String,
) -> Result<String, String> {
    require_principal(tx)?;
    require_account_role(tx, account_id, Role::Admin)?;

    if tx.db.account().id().find(&account_id).is_none() {
        return Err("account not found".to_string());
    }

    if entropy.len() < MIN_ENTROPY_LEN {
        return Err(format!(
            "entropy must be at least {MIN_ENTROPY_LEN} bytes (pass OS-random material from the client)"
        ));
    }

    let label = label.trim().to_string();
    let secret = generate_unique_token(tx, &entropy)?;

    let row = tx
        .db
        .account_token()
        .try_insert(AccountToken {
            id: 0,
            account_id,
            token: secret.clone(),
            label,
            created_by: tx.sender(),
            created_at: tx.timestamp,
        })
        .map_err(|e| format!("create_account_token failed: {e}"))?;

    log::info!(
        "create_account_token id={} account={} by={}",
        row.id,
        account_id,
        tx.sender()
    );
    Ok(secret)
}

/// Revoke one or more tokens on `account_id` by primary key.
///
/// Pass a single id or many. Ids that do not belong to the account are rejected.
/// Empty `token_ids` is a no-op.
#[reducer]
pub fn revoke_account_tokens(
    ctx: &ReducerContext,
    account_id: u64,
    token_ids: Vec<u64>,
) -> Result<(), String> {
    require_principal(ctx)?;
    require_account_role(ctx, account_id, Role::Admin)?;

    if ctx.db.account().id().find(&account_id).is_none() {
        return Err("account not found".to_string());
    }

    for token_id in token_ids {
        let Some(row) = ctx.db.account_token().id().find(&token_id) else {
            return Err(format!("token {token_id} not found"));
        };
        if row.account_id != account_id {
            return Err(format!("token {token_id} does not belong to this account"));
        }
        ctx.db.account_token().id().delete(&token_id);
        log::info!(
            "revoke_account_token id={} account={} by={}",
            token_id,
            account_id,
            ctx.sender()
        );
    }
    Ok(())
}

fn generate_unique_token(tx: &TxContext, entropy: &str) -> Result<String, String> {
    let mut rng = token_rng(tx, entropy);
    for _ in 0..TOKEN_GEN_ATTEMPTS {
        let candidate = generate_token_secret(&mut rng);
        if tx.db.account_token().token().find(&candidate).is_none() {
            return Ok(candidate);
        }
    }
    Err("failed to generate unique token".to_string())
}

fn generate_token_secret(rng: &mut StdRng) -> String {
    (0..TOKEN_LEN)
        .map(|_| {
            let i = rng.gen_range(0..TOKEN_CHARS.len());
            TOKEN_CHARS[i] as char
        })
        .collect()
}

/// Build a private `StdRng` for token generation.
///
/// STDB's `StdbRng` (`ctx.rng()`) always seeds from the call timestamp only
/// (`StdbRng::seed_from_ts` is private; no API to inject entropy). That is
/// public via `created_at`, so we never use it for secrets.
///
/// Instead we seed `rand::StdRng` from a 32-byte mix of:
/// - client `entropy` (primary secret)
/// - call timestamp + sender identity (domain separation, not secrecy)
fn token_rng(tx: &TxContext, entropy: &str) -> StdRng {
    let seed = mix_token_seed(
        tx.timestamp.to_micros_since_unix_epoch() as u64,
        &tx.sender().to_byte_array(),
        entropy.as_bytes(),
    );
    // Expand once so short entropy still floods the full seed state.
    let mut rng = StdRng::from_seed(seed);
    let mut expanded = [0u8; 32];
    rng.fill_bytes(&mut expanded);
    for (i, b) in seed.iter().enumerate() {
        expanded[i] ^= b;
    }
    StdRng::from_seed(expanded)
}

fn mix_token_seed(ts_micros: u64, sender: &[u8; 32], entropy: &[u8]) -> [u8; 32] {
    let mut seed = [0u8; 32];
    seed[0..8].copy_from_slice(&ts_micros.to_le_bytes());
    for (i, b) in sender.iter().enumerate() {
        seed[i % 32] ^= *b;
        seed[(i + 7) % 32] = seed[(i + 7) % 32].wrapping_add(b.wrapping_mul(31));
    }
    // FNV-ish absorb of client entropy across the seed.
    let mut h: u64 = 0xcbf29ce484222325;
    for (i, b) in entropy.iter().enumerate() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
        let idx = i % 32;
        seed[idx] ^= *b;
        seed[idx] = seed[idx].wrapping_add((h as u8).wrapping_add(i as u8));
        seed[(idx + 13) % 32] ^= (h >> 8) as u8;
        seed[(idx + 19) % 32] ^= (h >> 16) as u8;
    }
    seed[24..32].copy_from_slice(&h.to_le_bytes());
    seed
}

// ---------------------------------------------------------------------------
// HTTP handlers
// ---------------------------------------------------------------------------

/// Unauthenticated health check.
#[handler]
fn ping(_ctx: &mut HandlerContext, _req: Request) -> Response {
    text_response(http::StatusCode::OK, "pong")
}

/// Token-gated health check. Valid `Authorization` secret → `pong`.
#[handler]
fn account_ping(ctx: &mut HandlerContext, req: Request) -> Response {
    let Some(secret) = authorization_secret(&req) else {
        return text_response(http::StatusCode::UNAUTHORIZED, "missing authorization");
    };

    let found = ctx.with_tx(|tx| tx.db.account_token().token().find(&secret));
    match found {
        Some(acc) => text_response(
            http::StatusCode::OK,
            format!("valid token, label: {}", acc.label).as_str(),
        ),
        None => text_response(http::StatusCode::FORBIDDEN, "invalid token"),
    }
}

#[router]
fn router() -> Router {
    Router::new()
        .get("/ping", ping)
        .get("/account/ping", account_ping)
}

fn authorization_secret(req: &Request) -> Option<String> {
    let raw = req
        .headers()
        .get(http::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .trim();
    if raw.is_empty() {
        None
    } else {
        Some(raw.to_string())
    }
}

fn text_response(status: http::StatusCode, body: &str) -> Response {
    Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from_bytes(body.to_string()))
        .expect("valid HTTP response")
}
