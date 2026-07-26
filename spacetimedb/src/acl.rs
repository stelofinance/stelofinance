use crate::require_registered_user;
use crate::role_rank;
use crate::tables::*;
use spacetimedb::{Identity, ReducerContext, Table, reducer};

#[reducer]
pub fn grant_account_user(
    ctx: &ReducerContext,
    account_id: u64,
    username: String,
    role: UserRole,
) -> Result<(), String> {
    require_registered_user(ctx)?;

    let username = username.trim().to_string();
    if username.is_empty() {
        return Err("username required".to_string());
    }

    let account = load_account(ctx, account_id)?;
    let caller_role = caller_role_on(ctx, account_id)?;
    if role_rank(caller_role) < role_rank(UserRole::Admin) {
        return Err("admin or owner required".to_string());
    }

    let grantee = ctx
        .db
        .user()
        .bitcraft_username()
        .find(&username)
        .ok_or_else(|| "user not found".to_string())?;

    if grantee.id == ctx.sender() {
        return Err("cannot grant a role to yourself".to_string());
    }

    // Admins cannot assign Owner; only Owner can transfer ownership.
    if role == UserRole::Owner && caller_role != UserRole::Owner {
        return Err("only owner can grant owner".to_string());
    }

    let existing = find_membership(ctx, account_id, grantee.id);

    // Nobody but the current Owner may change or replace the Owner row.
    if let Some(ref m) = existing {
        if m.role == UserRole::Owner && role != UserRole::Owner {
            return Err("cannot demote owner; transfer ownership instead".to_string());
        }
        if m.role == UserRole::Owner && caller_role != UserRole::Owner {
            return Err("cannot modify owner".to_string());
        }
    }

    if role == UserRole::Owner {
        return transfer_ownership(ctx, &account, caller_role, &grantee, existing.as_ref());
    }

    // Non-owner grants: Admin or Owner assigning Read/Write/Admin.
    if caller_role != UserRole::Owner
        && existing.as_ref().is_some_and(|m| m.role == UserRole::Owner)
    {
        return Err("cannot modify owner".to_string());
    }

    upsert_membership(ctx, account_id, grantee.id, role, existing.as_ref())?;

    log::info!(
        "grant_account_user account={} user={} role={:?} by={}",
        account_id,
        username,
        role,
        ctx.sender()
    );
    Ok(())
}

#[reducer]
pub fn revoke_account_user(
    ctx: &ReducerContext,
    account_id: u64,
    username: String,
) -> Result<(), String> {
    require_registered_user(ctx)?;

    let username = username.trim().to_string();
    if username.is_empty() {
        return Err("username required".to_string());
    }

    let _account = load_account(ctx, account_id)?;
    let caller_role = caller_role_on(ctx, account_id)?;

    let target = ctx
        .db
        .user()
        .bitcraft_username()
        .find(&username)
        .ok_or_else(|| "user not found".to_string())?;

    let membership = find_membership(ctx, account_id, target.id)
        .ok_or_else(|| "user is not on this account".to_string())?;

    let leaving_self = target.id == ctx.sender();

    if leaving_self {
        if membership.role == UserRole::Owner {
            return Err("owner cannot leave; transfer ownership first".to_string());
        }
        // Any non-owner role may leave.
    } else {
        // Revoking someone else.
        if role_rank(caller_role) < role_rank(UserRole::Admin) {
            return Err("admin or owner required to revoke others".to_string());
        }
        if membership.role == UserRole::Owner {
            return Err("cannot revoke owner; transfer ownership first".to_string());
        }
        // Owner and Admin may revoke any non-owner.
    }

    // Exactly one Owner is preserved: we never delete an Owner row here.
    ctx.db.account_user().id().delete(&membership.id);

    log::info!(
        "revoke_account_user account={} user={} by={}",
        account_id,
        username,
        ctx.sender()
    );
    Ok(())
}

#[reducer]
pub fn set_account_primary(
    ctx: &ReducerContext,
    account_id: u64,
    primary: bool,
) -> Result<(), String> {
    require_registered_user(ctx)?;

    let mut account = load_account(ctx, account_id)?;
    let caller_role = caller_role_on(ctx, account_id)?;
    if caller_role != UserRole::Owner {
        return Err("only owner can set primary".to_string());
    }

    let sender = ctx.sender();

    if primary {
        if account.kind != AccountKind::Debit {
            return Err("only debit accounts can be primary".to_string());
        }

        // Must not already have a different primary wallet on this ledger.
        let conflict = ctx
            .db
            .account()
            .by_user_and_ledger()
            .filter((sender, account.ledger_id))
            .any(|a| a.id != account_id);
        if conflict {
            return Err("user already has a primary account on this ledger".to_string());
        }

        // If this account is already primary for caller, no-op success.
        if account.user_id == sender {
            return Ok(());
        }

        // Primary must be Owner or ZERO — clear any invalid non-owner primary.
        if account.user_id != Identity::ZERO && account.user_id != sender {
            return Err("invalid primary state".to_string());
        }

        account.user_id = sender;
        ctx.db.account().id().update(account);

        log::info!(
            "set_account_primary account={} primary=true by={}",
            account_id,
            sender
        );
    } else {
        if account.user_id == Identity::ZERO {
            return Ok(());
        }
        if account.user_id != sender {
            // Should be unreachable if only Owner is ever primary.
            return Err("invalid primary state".to_string());
        }
        account.user_id = Identity::ZERO;
        ctx.db.account().id().update(account);

        log::info!(
            "set_account_primary account={} primary=false by={}",
            account_id,
            sender
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

fn transfer_ownership(
    ctx: &ReducerContext,
    account: &Account,
    caller_role: UserRole,
    grantee: &User,
    grantee_membership: Option<&AccountUser>,
) -> Result<(), String> {
    if caller_role != UserRole::Owner {
        return Err("only owner can grant owner".to_string());
    }

    // Primary must be cleared before ownership can move.
    if account.user_id != Identity::ZERO {
        return Err("clear primary before transferring ownership".to_string());
    }

    let caller = ctx.sender();
    let caller_membership = find_membership(ctx, account.id, caller)
        .ok_or_else(|| "caller membership missing".to_string())?;

    // TODO, I think we already checked before that the caller is owner
    if caller_membership.role != UserRole::Owner {
        return Err("caller is not owner".to_string());
    }

    // Promote grantee to Owner (insert or update).
    upsert_membership(
        ctx,
        account.id,
        grantee.id,
        UserRole::Owner,
        grantee_membership,
    )?;

    // Demote previous Owner to Admin.
    let mut demoted = caller_membership;
    demoted.role = UserRole::Admin;
    demoted.updated_at = ctx.timestamp;
    ctx.db.account_user().id().update(demoted);

    // Ensure still exactly one Owner.
    let owner_count = count_owners(ctx, account.id);
    if owner_count != 1 {
        return Err("owner invariant violated".to_string());
    }

    log::info!(
        "transfer_ownership account={} new_owner={} by={}",
        account.id,
        grantee.bitcraft_username,
        caller
    );
    Ok(())
}

fn upsert_membership(
    ctx: &ReducerContext,
    account_id: u64,
    user_id: Identity,
    role: UserRole,
    existing: Option<&AccountUser>,
) -> Result<(), String> {
    match existing {
        Some(m) => {
            if m.role == role {
                return Ok(());
            }
            let mut updated = m.clone();
            updated.role = role;
            updated.updated_at = ctx.timestamp;
            ctx.db.account_user().id().update(updated);
        }
        None => {
            ctx.db.account_user().insert(AccountUser {
                id: 0,
                account_id,
                user_id,
                role,
                updated_at: ctx.timestamp,
                created_at: ctx.timestamp,
            });
        }
    }
    Ok(())
}

fn load_account(ctx: &ReducerContext, account_id: u64) -> Result<Account, String> {
    ctx.db
        .account()
        .id()
        .find(&account_id)
        .ok_or_else(|| "account not found".to_string())
}

fn caller_role_on(ctx: &ReducerContext, account_id: u64) -> Result<UserRole, String> {
    find_membership(ctx, account_id, ctx.sender())
        .map(|m| m.role)
        .ok_or_else(|| "not a member of this account".to_string())
}

fn find_membership(
    ctx: &ReducerContext,
    account_id: u64,
    user_id: Identity,
) -> Option<AccountUser> {
    ctx.db
        .account_user()
        .by_account_and_user()
        .filter((account_id, user_id))
        .next()
}

fn count_owners(ctx: &ReducerContext, account_id: u64) -> usize {
    ctx.db
        .account_user()
        .by_account_and_user()
        .filter(&account_id)
        .filter(|m| m.role == UserRole::Owner)
        .count()
}
