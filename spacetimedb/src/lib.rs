mod tables;

pub use tables::*;

use spacetimedb::{Identity, ReducerContext, Table, reducer};

const BITAUTH_ISSUER: &str = "https://auth.trinit.is/";
const BITAUTH_AUDIENCE: &str = "nintron-stelofinance";

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
    // Bypass everything for db owner
    if ctx.db.config().owner().find(ctx.sender()).is_some() {
        log::info!("owner connection allowed identity={}", ctx.sender());
        return Ok(());
    }

    // Regular clients: BitAuth only
    let jwt = ctx
        .sender_auth()
        .jwt()
        .ok_or_else(|| "authentication required: OIDC JWT missing".to_string())?;

    if jwt.issuer() != BITAUTH_ISSUER {
        return Err(format!(
            "invalid issuer: expected {BITAUTH_ISSUER}, got {}",
            jwt.issuer()
        ));
    }

    if !jwt.audience().iter().any(|a| a == BITAUTH_AUDIENCE) {
        return Err(format!(
            "invalid audience: expected {BITAUTH_AUDIENCE}, got {:?}",
            jwt.audience()
        ));
    }

    let identity = ctx.sender();
    if jwt.identity() != identity {
        return Err("token identity does not match connection sender".to_string());
    }

    let username = display_name_from_jwt(jwt)?;
    ensure_user(ctx, identity, username)?;
    Ok(())
}

/// App admin check for privileged reducers (ledgers, issuer accounts, audit, …).
#[allow(dead_code)] // used by admin reducers once they exist
pub fn require_admin(ctx: &ReducerContext) -> Result<(), String> {
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
