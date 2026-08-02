use crate::require_account_role;
use crate::require_principal;
use crate::tables::*;
use crate::transfers::{
    CreateTransferOutcome, TransferActor, create_transfer_core, finalize_transfer_core,
};
use crate::views::computed_balance;
use spacetimedb::http::{Body, HandlerContext, Request, Response, Router, handler, router};
use spacetimedb::{
    Identity, ProcedureContext, ReducerContext, Table, Timestamp, TxContext, procedure,
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
    ctx.try_with_tx(|tx| create_account_token_tx(tx, account_id, label.clone(), entropy.clone()))
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

/// Public account search (recipient lookup). No auth.
///
/// Query: `term` (required), `ledgerid` or `ledgerId` (required), optional `limit` (default 10, max 50).
/// Matches case-insensitive substring on address or primary username within the ledger.
#[handler]
fn search_accounts(ctx: &mut HandlerContext, req: Request) -> Response {
    let Some(term_raw) = parse_query_str(&req, "term") else {
        return json_err(http::StatusCode::BAD_REQUEST, "term required");
    };
    let term = term_raw.trim();
    if term.is_empty() {
        return json_err(http::StatusCode::BAD_REQUEST, "term required");
    }
    let term_upper = term.to_ascii_uppercase();

    let ledger_id = parse_query_u64(&req, "ledgerid")
        .or_else(|| parse_query_u64(&req, "ledgerId"));
    let Some(ledger_id) = ledger_id else {
        return json_err(http::StatusCode::BAD_REQUEST, "ledgerid required");
    };

    let limit = parse_query_u64(&req, "limit")
        .unwrap_or(DEFAULT_SEARCH_LIMIT)
        .min(MAX_SEARCH_LIMIT)
        .max(1);

    let result: Result<serde_json::Value, String> = ctx.try_with_tx(|tx| {
        if tx.db.ledger().id().find(&ledger_id).is_none() {
            return Err("ledger not found".to_string());
        }

        let mut out = Vec::new();
        for acc in tx.db.account().ledger_id().filter(&ledger_id) {
            if out.len() as u64 >= limit {
                break;
            }
            let username = if acc.user_id != Identity::ZERO {
                tx.db
                    .user()
                    .id()
                    .find(&acc.user_id)
                    .map(|u| u.bitcraft_username)
            } else {
                None
            };

            let addr_match = acc.address.to_ascii_uppercase().contains(&term_upper);
            let user_match = username
                .as_ref()
                .is_some_and(|u| u.to_ascii_uppercase().contains(&term_upper));
            if !addr_match && !user_match {
                continue;
            }

            out.push(serde_json::json!({
                "id": acc.id,
                "address": acc.address,
                "bitcraftUsername": username,
            }));
        }
        Ok(serde_json::Value::Array(out))
    });

    match result {
        Ok(body) => json_response(http::StatusCode::OK, body),
        Err(e) if e == "ledger not found" => json_err(http::StatusCode::BAD_REQUEST, &e),
        Err(e) => json_err(http::StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

/// Token-gated health check.
#[handler]
fn account_ping(ctx: &mut HandlerContext, req: Request) -> Response {
    match require_token(ctx, &req) {
        Ok(tkn) => text_response(
            http::StatusCode::OK,
            format!("pong, acc id: {}", tkn.account_id).as_str(),
        ),
        Err(resp) => resp,
    }
}

/// Account details for the token's account.
#[handler]
fn get_account(ctx: &mut HandlerContext, req: Request) -> Response {
    let token = match require_token(ctx, &req) {
        Ok(t) => t,
        Err(resp) => return resp,
    };

    let result: Result<serde_json::Value, String> = ctx.try_with_tx(|tx| {
        let acc = tx
            .db
            .account()
            .id()
            .find(&token.account_id)
            .ok_or_else(|| "account not found".to_string())?;
        Ok(account_to_json(&acc))
    });
    match result {
        Ok(body) => json_response(http::StatusCode::OK, body),
        Err(e) => json_err(http::StatusCode::NOT_FOUND, &e),
    }
}

const DEFAULT_TRANSFER_LIMIT: u64 = 100;
const MAX_TRANSFER_LIMIT: u64 = 1000;
const DEFAULT_SEARCH_LIMIT: u64 = 10;
const MAX_SEARCH_LIMIT: u64 = 50;

/// List transfers involving the token's account (`limit` / `offset` query params).
// TODO: Clean this up
#[handler]
fn list_transfers(ctx: &mut HandlerContext, req: Request) -> Response {
    let token = match require_token(ctx, &req) {
        Ok(t) => t,
        Err(resp) => return resp,
    };

    let limit = parse_query_u64(&req, "limit")
        .unwrap_or(DEFAULT_TRANSFER_LIMIT)
        .min(MAX_TRANSFER_LIMIT)
        .max(1);
    let offset = parse_query_u64(&req, "offset").unwrap_or(0);

    let result: Result<serde_json::Value, String> = ctx.try_with_tx(|tx| {
        let account_id = token.account_id;
        let mut rows: Vec<Transfer> = tx
            .db
            .transfer()
            .debit_account_id()
            .filter(&account_id)
            .chain(tx.db.transfer().credit_account_id().filter(&account_id))
            .collect();

        // Deduplicate if both legs somehow match (shouldn't for distinct accounts, so maybe we could remove).
        rows.sort_by(|a, b| a.id.cmp(&b.id));
        rows.dedup_by_key(|t| t.id);

        // Newest first: created_at desc, then id desc.
        rows.sort_by(|a, b| {
            let ta = a.created_at.to_micros_since_unix_epoch();
            let tb = b.created_at.to_micros_since_unix_epoch();
            tb.cmp(&ta).then_with(|| b.id.cmp(&a.id))
        });

        let page: Vec<serde_json::Value> = rows
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .map(|t| transfer_to_json(tx, &t))
            .collect();
        Ok(serde_json::Value::Array(page))
    });
    match result {
        Ok(body) => json_response(http::StatusCode::OK, body),
        Err(e) => json_err(http::StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

/// Create a transfer (token account = sender).
#[handler]
fn post_transfer(ctx: &mut HandlerContext, req: Request) -> Response {
    let token = match require_token(ctx, &req) {
        Ok(t) => t,
        Err(resp) => return resp,
    };

    let idem_key = match req
        .headers()
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        Some(k) => k,
        None => {
            return json_err(
                http::StatusCode::BAD_REQUEST,
                "Idempotency-Key header required",
            );
        }
    };

    let body_bytes = req.into_body().into_bytes();
    let input = match parse_create_transfer_body(&body_bytes) {
        Ok(v) => v,
        Err(e) => return json_err(http::StatusCode::BAD_REQUEST, &e),
    };

    let receiving_id = input.receiving_id;
    let amount = input.amount;
    let memo = input.memo;
    let pending = input.pending;

    if amount < 1 {
        return json_err(http::StatusCode::BAD_REQUEST, "amount must be >= 1");
    }

    let result: Result<(CreateTransferOutcome, serde_json::Value), String> =
        ctx.try_with_tx(|tx| {
            // TxContext derefs to ReducerContext for shared domain core.
            let outcome = create_transfer_core(
                tx,
                TransferActor::TokenAccount(token.account_id),
                token.account_id,
                receiving_id,
                amount,
                memo.clone(),
                idem_key.clone(),
                pending,
            )?;
            let json = transfer_to_json(tx, &outcome.transfer);
            Ok((outcome, json))
        });
    match result {
        Ok((CreateTransferOutcome { replay, .. }, json)) => {
            let status = if replay {
                http::StatusCode::OK
            } else {
                http::StatusCode::CREATED
            };
            json_response(status, json)
        }
        Err(e) => map_domain_err(&e),
    }
}

#[handler]
fn put_transfer(ctx: &mut HandlerContext, req: Request) -> Response {
    let token = match require_token(ctx, &req) {
        Ok(t) => t,
        Err(resp) => return resp,
    };

    let body_bytes = req.into_body().into_bytes();
    let input = match parse_finalize_transfer_body(&body_bytes) {
        Ok(v) => v,
        Err(e) => return json_err(http::StatusCode::BAD_REQUEST, &e),
    };

    let result: Result<serde_json::Value, String> = ctx.try_with_tx(|tx| {
        let transfer = finalize_transfer_core(
            tx,
            TransferActor::TokenAccount(token.account_id),
            input.transfer_id,
            input.amount,
        )?;
        Ok(transfer_to_json(tx, &transfer))
    });
    match result {
        Ok(json) => json_response(http::StatusCode::OK, json),
        Err(e) => map_domain_err(&e),
    }
}

#[router]
fn router() -> Router {
    Router::new()
        .get("/ping", ping)
        .get("/accounts", search_accounts)
        .get("/account/ping", account_ping)
        .get("/account", get_account)
        .get("/account/transfers", list_transfers)
        .post("/account/transfers", post_transfer)
        .put("/account/transfer", put_transfer)
}

// ---------------------------------------------------------------------------
// HTTP request DTOs (manual JSON parse — avoids serde_derive / host C toolchain)
// ---------------------------------------------------------------------------

struct CreateTransferBody {
    receiving_id: u64,
    amount: u64,
    memo: Option<String>,
    pending: bool,
}

struct FinalizeTransferBody {
    transfer_id: u64,
    amount: u64,
}

// TODO: Clean this up
fn parse_create_transfer_body(bytes: &[u8]) -> Result<CreateTransferBody, String> {
    let v: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "invalid JSON body".to_string())?;
    let receiving_id = v
        .get("receivingId")
        .and_then(|x| x.as_u64())
        .ok_or_else(|| "receivingId required".to_string())?;
    let amount = v
        .get("amount")
        .and_then(|x| x.as_u64())
        .ok_or_else(|| "amount required".to_string())?;
    let memo = match v.get("memo") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(_) => return Err("memo must be a string".to_string()),
    };
    let pending = v.get("pending").and_then(|x| x.as_bool()).unwrap_or(false);
    Ok(CreateTransferBody {
        receiving_id,
        amount,
        memo,
        pending,
    })
}

// TODO: Clean this up
fn parse_finalize_transfer_body(bytes: &[u8]) -> Result<FinalizeTransferBody, String> {
    let v: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "invalid JSON body".to_string())?;
    let transfer_id = v
        .get("transferId")
        .and_then(|x| x.as_u64())
        .ok_or_else(|| "transferId required".to_string())?;
    let amount = v
        .get("amount")
        .and_then(|x| x.as_u64())
        .ok_or_else(|| "amount required".to_string())?;
    Ok(FinalizeTransferBody {
        transfer_id,
        amount,
    })
}

// ---------------------------------------------------------------------------
// Auth + JSON helpers
// ---------------------------------------------------------------------------

fn require_token(ctx: &mut HandlerContext, req: &Request) -> Result<AccountToken, Response> {
    let Some(secret) = authorization_secret(req) else {
        return Err(json_err(
            http::StatusCode::UNAUTHORIZED,
            "missing authorization",
        ));
    };
    match ctx.with_tx(|tx| tx.db.account_token().token().find(&secret)) {
        Some(tok) => Ok(tok),
        None => Err(json_err(http::StatusCode::FORBIDDEN, "invalid token")),
    }
}

/// Authorization header is the raw token secret only (no `Bearer ` prefix).
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

fn parse_query_str(req: &Request, name: &str) -> Option<String> {
    let query = req.uri().query()?;
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        let k = parts.next()?;
        let v = parts.next().unwrap_or("");
        if k == name {
            // Minimal percent-decoding for spaces (+ / %20) so terms work from browsers.
            return Some(percent_decode(v));
        }
    }
    None
}

fn parse_query_u64(req: &Request, name: &str) -> Option<u64> {
    parse_query_str(req, name)?.parse().ok()
}

fn percent_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let h = |c: u8| -> Option<u8> {
                    match c {
                        b'0'..=b'9' => Some(c - b'0'),
                        b'a'..=b'f' => Some(c - b'a' + 10),
                        b'A'..=b'F' => Some(c - b'A' + 10),
                        _ => None,
                    }
                };
                if let (Some(hi), Some(lo)) = (h(bytes[i + 1]), h(bytes[i + 2])) {
                    out.push((hi << 4 | lo) as char);
                    i += 3;
                } else {
                    out.push('%');
                    i += 1;
                }
            }
            c => {
                out.push(c as char);
                i += 1;
            }
        }
    }
    out
}

fn account_to_json(acc: &Account) -> serde_json::Value {
    let kind = match acc.kind {
        AccountKind::Debit => "Debit",
        AccountKind::Credit => "Credit",
    };
    serde_json::json!({
        "id": acc.id,
        "address": acc.address,
        "kind": kind,
        "balance": computed_balance(acc),
        "debitsPending": acc.debits_pending,
        "debitsPosted": acc.debits_posted,
        "creditsPending": acc.credits_pending,
        "creditsPosted": acc.credits_posted,
        "ledgerId": acc.ledger_id,
        "isPrimary": acc.user_id != Identity::ZERO,
        "createdAt": timestamp_rfc3339(acc.created_at),
    })
}

fn transfer_to_json(tx: &TxContext, tr: &Transfer) -> serde_json::Value {
    let debit_addr = tx
        .db
        .account()
        .id()
        .find(&tr.debit_account_id)
        .map(|a| a.address)
        .unwrap_or_default();
    let credit_addr = tx
        .db
        .account()
        .id()
        .find(&tr.credit_account_id)
        .map(|a| a.address)
        .unwrap_or_default();

    let amount = match tr.state {
        TransferState::Pending => tr.pending_amount.unwrap_or(0),
        TransferState::Posted | TransferState::PostPending | TransferState::VoidPending => {
            tr.posted_amount.unwrap_or(0)
        }
    };

    serde_json::json!({
        "id": tr.id,
        "debitAccId": tr.debit_account_id,
        "creditAccId": tr.credit_account_id,
        "pendingAmount": tr.pending_amount,
        "postedAmount": tr.posted_amount,
        "amount": amount,
        "ledgerId": tr.ledger_id,
        "debitAddr": debit_addr,
        "creditAddr": credit_addr,
        "kind": transfer_kind_str(tr.kind),
        "state": transfer_state_str(tr.state),
        "memo": tr.memo,
        "createdAt": timestamp_rfc3339(tr.created_at),
        "finalizedAt": tr.finalized_at.map(timestamp_rfc3339),
    })
}

fn transfer_kind_str(kind: TransferKind) -> &'static str {
    match kind {
        TransferKind::Liability => "Liability",
        TransferKind::Asset => "Asset",
        TransferKind::Issue => "Issue",
        TransferKind::Redeem => "Redeem",
    }
}

fn transfer_state_str(state: TransferState) -> &'static str {
    match state {
        TransferState::Posted => "Posted",
        TransferState::Pending => "Pending",
        TransferState::PostPending => "PostPending",
        TransferState::VoidPending => "VoidPending",
    }
}

fn timestamp_rfc3339(ts: Timestamp) -> String {
    ts.to_rfc3339().unwrap_or_else(|_| format!("{ts}"))
}

fn map_domain_err(e: &str) -> Response {
    let lower = e.to_ascii_lowercase();
    if lower.contains("idempotency key conflict") || lower.contains("finalize idempotency conflict")
    {
        return json_err(http::StatusCode::CONFLICT, e);
    }
    if lower.contains("not found") || lower.contains("missing") {
        return json_err(http::StatusCode::NOT_FOUND, e);
    }
    if lower.contains("insufficient")
        || lower.contains("permission")
        || lower.contains("requires token")
    {
        return json_err(http::StatusCode::FORBIDDEN, e);
    }
    json_err(http::StatusCode::BAD_REQUEST, e)
}

fn json_response(status: http::StatusCode, body: serde_json::Value) -> Response {
    let bytes = serde_json::to_vec(&body).unwrap_or_else(|_| b"{}".to_vec());
    Response::builder()
        .status(status)
        .header(
            http::header::CONTENT_TYPE,
            "application/json; charset=utf-8",
        )
        .body(Body::from_bytes(bytes))
        .expect("valid HTTP response")
}

fn json_err(status: http::StatusCode, message: &str) -> Response {
    json_response(status, serde_json::json!({ "error": message }))
}

fn text_response(status: http::StatusCode, body: &str) -> Response {
    Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from_bytes(body.to_string()))
        .expect("valid HTTP response")
}
