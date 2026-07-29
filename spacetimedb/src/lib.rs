mod acl;
mod apps;
mod tables;
mod transfers;
mod views;
mod webhooks;

pub use tables::*;

use spacetimedb::{Identity, ReducerContext, Table, rand::Rng, reducer};

// OIDC configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OidcProvider {
    BitAuth,
    SpacetimeAuth,
}

impl OidcProvider {
    pub const fn issuer(self) -> &'static str {
        match self {
            Self::BitAuth => "https://auth.trinit.is/",
            Self::SpacetimeAuth => "https://auth.spacetimedb.com/oidc",
        }
    }

    pub const fn audience(self) -> &'static str {
        match self {
            Self::BitAuth => "nintron-stelofinance",
            Self::SpacetimeAuth => "client_033wW7fObq5GPPc4ESCFsF",
        }
    }

    /// Try to turn an issuer string into the enum
    pub fn from_issuer(issuer: &str) -> Option<Self> {
        match issuer {
            "https://auth.trinit.is/" => Some(Self::BitAuth),
            "https://auth.spacetimedb.com/oidc" => Some(Self::SpacetimeAuth),
            _ => None,
        }
    }

    /// Validate both issuer (already known) and audience
    pub fn is_valid_audience(self, audience: &str) -> bool {
        self.audience() == audience
    }
}

/// Easy to read / hard to misread letters
const ADDRESS_STD_CHARS: &[u8] = b"ABCDEFGHJKMNPRTUVWXY";
const MAX_ADDRESS_LENGTH: usize = 16;
const DEFAULT_ADDRESS_LENGTH: usize = 8;
const ADDRESS_GEN_ATTEMPTS: usize = 4;

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

#[reducer(client_connected)]
pub fn client_connected(ctx: &ReducerContext) -> Result<(), String> {
    let identity = ctx.sender();
    let auth_ctx = ctx.sender_auth();

    // DB owner bypasses all
    if ctx.db.config().owner().find(&identity).is_some() {
        log::info!("owner connection allowed identity={identity}");
        return Ok(());
    }

    // JWT required
    let jwt = auth_ctx
        .jwt()
        .ok_or_else(|| "OIDC JWT missing".to_string())?;
    let provider = {
        let provider = OidcProvider::from_issuer(jwt.issuer())
            .ok_or_else(|| "invalid OIDC provider".to_string())?;
        jwt.audience()
            .iter()
            .any(|a| provider.is_valid_audience(a))
            .then_some(provider)
            .ok_or_else(|| "invalid OIDC audience".to_string())?
    };

    // Now handle users vs apps
    return match provider {
        OidcProvider::BitAuth => {
            let username = display_name_from_jwt(jwt)?;
            ensure_user(ctx, identity, username)?;
            Ok(())
        }
        OidcProvider::SpacetimeAuth => {
            if ctx.db.app().id().find(&identity).is_some() {
                return Ok(());
            }

            if ctx.db.user().id().find(&identity).is_some() {
                return Err("identity cannot be both user and app".to_string());
            }
            apps::try_fulfill_app_ticket(ctx, identity, jwt.subject())?;
            log::info!("app ticket fulfilled identity={identity}");
            return Ok(());
        }
    };
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
        AccountKind::Credit => {
            require_admin(ctx)?; // Must be admin

            // Credit accounts cannot be a user's primary wallet.
            if is_primary {
                return Err("credit accounts cannot be primary".to_string());
            }
        }
        AccountKind::Debit => {}
    }

    // Custom address is admin-only; None (or blank) -> auto-generate.
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

/// Ordering for `Role` comparisons (Read < Write < Admin < Owner).
pub(crate) fn role_rank(role: Role) -> u8 {
    match role {
        Role::Read => 1,
        Role::Write => 2,
        Role::Admin => 3,
        Role::Owner => 4,
    }
}

pub(crate) fn require_admin(ctx: &ReducerContext) -> Result<(), String> {
    let user = ctx
        .db
        .user()
        .id()
        .find(&ctx.sender())
        .ok_or_else(|| "not a registered user".to_string())?;
    if !user.is_admin {
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
// TODO: Technically we don't delete apps or users, so if the client even connected they
// MUST exist... I guess it's extra safety for now
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

/// Check if the caller has at least the minimum role
pub(crate) fn has_minimum_role(ctx: &ReducerContext, account_id: u64, min: Role) -> bool {
    effective_role(ctx, account_id).is_some_and(|r| role_rank(r) >= role_rank(min))
}

pub(crate) fn require_account_role(
    ctx: &ReducerContext,
    account_id: u64,
    min: Role,
) -> Result<(), String> {
    if has_minimum_role(ctx, account_id, min) {
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

/// `None` -> random 8-char address.
/// `Some` -> uppercase A–Z only. Unique within ledger.
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
