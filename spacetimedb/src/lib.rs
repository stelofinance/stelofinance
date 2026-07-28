mod acl;
mod apps;
mod tables;
mod transfers;
mod views;
mod webhooks;

pub use tables::*;

use spacetimedb::{Identity, ReducerContext, Table, rand::Rng, reducer};

const BITAUTH_ISSUER: &str = "https://auth.trinit.is/";
const BITAUTH_AUDIENCE: &str = "nintron-stelofinance";

/// SpacetimeAuth OIDC (anonymous app identities). Set to your project client id.
const SPACETIMEAUTH_ISSUER: &str = "https://auth.spacetimedb.com/oidc";
const SPACETIMEAUTH_CLIENT_ID: &str = "client_033wW7fObq5GPPc4ESCFsF";

/// Easy to read / hard to misread letters
const ADDRESS_STD_CHARS: &[u8] = b"ABCDEFGHJKMNPRTUVWXY";
const MAX_ADDRESS_LENGTH: usize = 16;
const DEFAULT_ADDRESS_LENGTH: usize = 8;
const ADDRESS_GEN_ATTEMPTS: usize = 8;

#[reducer(init)]
pub fn init(ctx: &ReducerContext) -> Result<(), String> {
    ctx.db.config().try_insert(Config {
        owner: ctx.sender(),
    })?;
    log::info!(
        "stelofinance module initialized; owner identity = {}",
        ctx.sender()
    );
    Ok(())
}

/// Runs after the host validates credentials and assigns `ctx.sender()`.
#[reducer(client_connected)]
pub fn client_connected(ctx: &ReducerContext) -> Result<(), String> {
    let identity = ctx.sender();

    // Bypass everything for db owner
    if ctx.db.config().owner().find(&identity).is_some() {
        log::info!("owner connection allowed identity={identity}");
        return Ok(());
    }

    // Already-registered apps (any prior SpacetimeAuth bind). Mutually exclusive with users.
    if ctx.db.app().id().find(&identity).is_some() {
        if ctx.db.user().id().find(&identity).is_some() {
            return Err("identity cannot be both user and app".to_string());
        }
        log::info!("app connection allowed identity={identity}");
        return Ok(());
    }

    let jwt = ctx
        .sender_auth()
        .jwt()
        .ok_or_else(|| "authentication required: OIDC JWT missing".to_string())?;

    if jwt.identity() != identity {
        return Err("token identity does not match connection sender".to_string());
    }

    // SpacetimeAuth: fulfill open app ticket by OIDC `sub`, or reject.
    if jwt.issuer() == SPACETIMEAUTH_ISSUER {
        if !jwt.audience().iter().any(|a| a == SPACETIMEAUTH_CLIENT_ID) {
            return Err(format!(
                "invalid audience: expected {SPACETIMEAUTH_CLIENT_ID}, got {:?}",
                jwt.audience()
            ));
        }
        if ctx.db.user().id().find(&identity).is_some() {
            return Err("identity cannot be both user and app".to_string());
        }
        apps::try_fulfill_app_ticket(ctx, identity, jwt.subject())?;
        log::info!("app ticket fulfilled identity={identity}");
        return Ok(());
    }

    // Human clients: BitAuth only
    if jwt.issuer() != BITAUTH_ISSUER {
        return Err(format!(
            "invalid issuer: expected BitAuth or SpacetimeAuth, got {}",
            jwt.issuer()
        ));
    }

    if !jwt.audience().iter().any(|a| a == BITAUTH_AUDIENCE) {
        return Err(format!(
            "invalid audience: expected {BITAUTH_AUDIENCE}, got {:?}",
            jwt.audience()
        ));
    }

    let username = display_name_from_jwt(jwt)?;
    ensure_user(ctx, identity, username)?;
    Ok(())
}

#[reducer]
pub fn create_ledger(
    ctx: &ReducerContext,
    name: String,
    scale: u8,
    kind: LedgerKind,
) -> Result<(), String> {
    require_admin(ctx)?; // Admin only, obviously

    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("ledger name required".to_string());
    }

    let ledger = ctx
        .db
        .ledger()
        .try_insert(Ledger {
            id: 0,
            name: name.clone(),
            scale,
            kind,
        })
        .map_err(|e| format!("create_ledger failed: {e}"))?;

    log::info!(
        "created ledger id={} name={} scale={} by {}",
        ledger.id,
        name,
        scale,
        ctx.sender()
    );
    Ok(())
}

#[reducer]
pub fn create_account(
    ctx: &ReducerContext,
    ledger_id: u64,
    kind: AccountKind,
    address: Option<String>,
    webhook: Option<String>,
    is_primary: bool,
) -> Result<(), String> {
    require_registered_user(ctx)?;

    match kind {
        AccountKind::Credit => require_admin(ctx)?,
        AccountKind::Debit => {}
    }

    // Credit (issuer/liability) accounts cannot be a user's primary wallet.
    if is_primary && matches!(kind, AccountKind::Credit) {
        return Err("credit accounts cannot be primary".to_string());
    }

    // Custom address is admin-only; None (or blank) → auto-generate.
    let address_input = match address {
        Some(a) if !a.trim().is_empty() => {
            require_admin(ctx)?;
            Some(a)
        }
        Some(_) | None => None,
    };

    if ctx.db.ledger().id().find(&ledger_id).is_none() {
        return Err("ledger not found".to_string());
    }

    let address = normalize_address(ctx, ledger_id, address_input)?;
    let webhook = normalize_webhook(webhook)?;

    // Ensure if they want to set primary, they have no other primary on this ledger.
    let sender = ctx.sender();
    if is_primary {
        let already_primary = ctx
            .db
            .account()
            .by_user_and_ledger()
            .filter((sender, ledger_id))
            .next()
            .is_some();
        if already_primary {
            return Err("user already has a primary account on this ledger".to_string());
        }
    }

    let account = ctx
        .db
        .account()
        .try_insert(Account {
            id: 0,
            address: address.clone(),
            webhook,
            user_id: if is_primary { sender } else { Identity::ZERO },
            debits_pending: 0,
            debits_posted: 0,
            credits_pending: 0,
            credits_posted: 0,
            ledger_id,
            kind,
            created_at: ctx.timestamp,
        })
        .map_err(|e| format!("create_account failed: {e}"))?;

    ctx.db.account_member().insert(AccountMember {
        id: 0,
        account_id: account.id,
        member_id: sender,
        kind: MemberKind::User,
        role: Role::Owner,
        updated_at: ctx.timestamp,
        created_at: ctx.timestamp,
    });

    log::info!(
        "created account id={} address={} ledger={} kind={:?} primary={} owner={}",
        account.id,
        address,
        ledger_id,
        kind,
        is_primary,
        sender
    );
    Ok(())
}

/// App admin: set an account's payment address by id.
/// Address must be non-empty A–Z (after trim/uppercase), max length, unique within ledger.
#[reducer]
pub fn update_account_address(
    ctx: &ReducerContext,
    account_id: u64,
    address: String,
) -> Result<(), String> {
    require_admin(ctx)?;

    let mut account = ctx
        .db
        .account()
        .id()
        .find(&account_id)
        .ok_or_else(|| "account not found".to_string())?;

    let new_address = parse_custom_address(&address)?;
    if new_address == account.address {
        return Ok(());
    }
    if address_taken(ctx, account.ledger_id, &new_address) {
        return Err("address already taken".to_string());
    }

    let old = account.address.clone();
    account.address = new_address.clone();
    ctx.db.account().id().update(account);

    log::info!(
        "update_account_address account={} {} → {} by={}",
        account_id,
        old,
        new_address,
        ctx.sender()
    );
    Ok(())
}

/// Whether a user row has the app-admin flag (`User.is_admin`).
pub(crate) fn is_admin(user: &User) -> bool {
    user.is_admin
}

/// Ordering for `Role` comparisons (Read < Write < Admin < Owner).
pub(crate) fn role_rank(role: Role) -> u8 {
    match role {
        Role::Read => 1,
        Role::Write => 2,
        Role::Admin => 3,
        Role::Owner => 4,
    }
}

/// App admin check for privileged reducers (ledgers, issuer accounts, audit, …).
pub(crate) fn require_admin(ctx: &ReducerContext) -> Result<(), String> {
    let user = ctx
        .db
        .user()
        .id()
        .find(&ctx.sender())
        .ok_or_else(|| "not a registered user".to_string())?;
    if !is_admin(&user) {
        return Err("admin required".to_string());
    }
    Ok(())
}

/// Human player only (BitAuth `user` row).
pub(crate) fn require_registered_user(ctx: &ReducerContext) -> Result<(), String> {
    if ctx.db.user().id().find(&ctx.sender()).is_none() {
        return Err("not a registered user".to_string());
    }
    Ok(())
}

/// Registered human **or** app principal.
pub(crate) fn require_principal(ctx: &ReducerContext) -> Result<(), String> {
    let sender = ctx.sender();
    if ctx.db.user().id().find(&sender).is_some() {
        return Ok(());
    }
    if ctx.db.app().id().find(&sender).is_some() {
        return Ok(());
    }
    Err("not a registered user or app".to_string())
}

/// Account role for `ctx.sender()` from `account_member`.
pub(crate) fn effective_role(ctx: &ReducerContext, account_id: u64) -> Option<Role> {
    let sender = ctx.sender();
    ctx.db
        .account_member()
        .by_account_and_member()
        .filter((account_id, sender))
        .next()
        .map(|m| m.role)
}

pub(crate) fn has_account_role(ctx: &ReducerContext, account_id: u64, min: Role) -> bool {
    effective_role(ctx, account_id).is_some_and(|r| role_rank(r) >= role_rank(min))
}

pub(crate) fn require_account_role(
    ctx: &ReducerContext,
    account_id: u64,
    min: Role,
) -> Result<(), String> {
    if has_account_role(ctx, account_id, min) {
        Ok(())
    } else {
        Err("insufficient account permission".to_string())
    }
}

fn ensure_user(ctx: &ReducerContext, identity: Identity, username: String) -> Result<(), String> {
    match ctx.db.user().id().find(&identity) {
        Some(existing) => {
            if existing.bitcraft_username != username {
                let updated = User {
                    bitcraft_username: username.clone(),
                    ..existing.clone()
                };
                if ctx.db.user().bitcraft_username().find(&username).is_none() {
                    ctx.db.user().id().update(updated);
                    log::info!(
                        "user {} renamed display name → {}",
                        existing.bitcraft_username,
                        username
                    );
                } else {
                    log::warn!(
                        "user {:?} display name {} already taken; leaving {}",
                        identity,
                        username,
                        existing.bitcraft_username
                    );
                }
            }
        }
        None => {
            ctx.db.user().insert(User {
                id: identity,
                bitcraft_username: username.clone(),
                is_admin: false,
                created_at: ctx.timestamp,
            });
            log::info!("created user {username} for identity {identity}");
        }
    }
    Ok(())
}

/// BitAuth display name from `preferred_username` only.
fn display_name_from_jwt(jwt: &spacetimedb::JwtClaims) -> Result<String, String> {
    let payload: serde_json::Value = serde_json::from_str(jwt.raw_payload())
        .map_err(|e| format!("invalid JWT payload JSON: {e}"))?;

    let username = payload
        .get("preferred_username")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "missing preferred_username in token claims".to_string())?;

    Ok(username.to_string())
}

/// `None` → random 8-char address. `Some` → uppercase A–Z only. Unique within ledger.
fn normalize_address(
    ctx: &ReducerContext,
    ledger_id: u64,
    address: Option<String>,
) -> Result<String, String> {
    let Some(raw) = address else {
        return generate_unique_address(ctx, ledger_id);
    };

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return generate_unique_address(ctx, ledger_id);
    }

    let upper = parse_custom_address(trimmed)?;
    if address_taken(ctx, ledger_id, &upper) {
        return Err("address already taken".to_string());
    }
    Ok(upper)
}

/// Non-empty A–Z only (trim + uppercase). Used for create custom address and updates.
fn parse_custom_address(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("address required".to_string());
    }
    if trimmed.len() > MAX_ADDRESS_LENGTH {
        return Err(format!("address exceeds max length ({MAX_ADDRESS_LENGTH})"));
    }
    let upper = trimmed.to_ascii_uppercase();
    if !upper.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err("invalid account configuration: address must be A–Z only".to_string());
    }
    Ok(upper)
}

fn generate_unique_address(ctx: &ReducerContext, ledger_id: u64) -> Result<String, String> {
    for _ in 0..ADDRESS_GEN_ATTEMPTS {
        let candidate = generate_address(ctx);
        if !address_taken(ctx, ledger_id, &candidate) {
            return Ok(candidate);
        }
    }
    Err("failed to generate unique address".to_string())
}

fn address_taken(ctx: &ReducerContext, ledger_id: u64, address: &str) -> bool {
    ctx.db
        .account()
        .address()
        .filter(&address.to_string())
        .any(|acc| acc.ledger_id == ledger_id)
}

fn generate_address(ctx: &ReducerContext) -> String {
    let mut rng = ctx.rng();
    (0..DEFAULT_ADDRESS_LENGTH)
        .map(|_| {
            let i = rng.gen_range(0..ADDRESS_STD_CHARS.len());
            ADDRESS_STD_CHARS[i] as char
        })
        .collect()
}

/// Validate webhook URL: `None`/blank clears; otherwise require absolute `http(s)://…`.
pub(crate) fn normalize_webhook(webhook: Option<String>) -> Result<Option<String>, String> {
    let Some(raw) = webhook else {
        return Ok(None);
    };
    let w = raw.trim();
    if w.is_empty() {
        return Ok(None);
    }
    if !(w.starts_with("http://") || w.starts_with("https://")) {
        return Err("webhook must be an absolute http(s) URL".to_string());
    }
    // Minimal absolute-URI check (Go uses url.ParseRequestURI).
    let rest = w
        .split_once("://")
        .map(|(_, r)| r)
        .filter(|r| !r.is_empty());
    if rest.is_none() {
        return Err("webhook must be a valid absolute URL".to_string());
    }
    Ok(Some(w.to_string()))
}
