use crate::effective_role;
use crate::normalize_webhook;
use crate::require_principal;
use crate::require_registered_user;
use crate::role_rank;
use crate::tables::*;
use spacetimedb::{Identity, ReducerContext, Table, reducer};

/// Grant or update a member's role on an account.
///
/// `member_id` is resolved as a **user** (if present in `user`) else **app** (if present in `app`).
///
/// Owner may only be granted to users
/// Admin is max role for apps
#[reducer]
pub fn grant_account_member(
    ctx: &ReducerContext,
    account_id: u64,
    member_id: Identity,
    role: Role,
) -> Result<(), String> {
    require_principal(ctx)?;

    if member_id == ctx.sender() {
        return Err("cannot grant a role to yourself".to_string());
    }

    let account = load_account(ctx, account_id)?;
    let caller_role = caller_role_on(ctx, account_id)?;
    if role_rank(caller_role) < role_rank(Role::Admin) {
        return Err("admin or owner required".to_string());
    }

    let kind = resolve_member_kind(ctx, member_id)?;

    if kind == MemberKind::App && role == Role::Owner {
        return Err("apps cannot be granted owner".to_string());
    }

    // Admins cannot assign Owner; only Owner can transfer ownership.
    if role == Role::Owner && caller_role != Role::Owner {
        return Err("only owner can grant owner".to_string());
    }

    let existing = find_membership(ctx, account_id, member_id);

    if let Some(ref e) = existing {
        if e.kind != kind {
            return Err("member kind mismatch with existing membership".to_string());
        }
        if e.role == Role::Owner || (role == Role::Owner && caller_role != Role::Owner) {
            return Err("cannot modify owner".to_string());
        }
    }

    if role == Role::Owner {
        // Only users can become Owner (kind already restricted above for App).
        let grantee = ctx
            .db
            .user()
            .id()
            .find(&member_id)
            .ok_or_else(|| "user not found".to_string())?;
        return transfer_ownership(ctx, &account, caller_role, &grantee, existing.as_ref());
    }

    // Non-owner grants: Admin or Owner assigning Read/Write/Admin.
    if caller_role != Role::Owner && existing.as_ref().is_some_and(|m| m.role == Role::Owner) {
        return Err("cannot modify owner".to_string());
    }

    upsert_membership(ctx, account_id, member_id, kind, role, existing.as_ref())?;

    log::info!(
        "grant_account_member account={} member={} kind={:?} role={:?} by={}",
        account_id,
        member_id,
        kind,
        role,
        ctx.sender()
    );
    Ok(())
}

/// Revoke a member from an account by Identity. Members may leave themselves (except Owner).
#[reducer]
pub fn revoke_account_member(
    ctx: &ReducerContext,
    account_id: u64,
    member_id: Identity,
) -> Result<(), String> {
    require_principal(ctx)?;

    let _account = load_account(ctx, account_id)?;
    let caller_role = caller_role_on(ctx, account_id)?;

    // Ensure principal exists as user or app (clearer error than "not on account").
    let _kind = resolve_member_kind(ctx, member_id)?;

    let membership = find_membership(ctx, account_id, member_id)
        .ok_or_else(|| "member is not on this account".to_string())?;

    let leaving_self = member_id == ctx.sender();

    if leaving_self {
        if membership.role == Role::Owner {
            return Err("owner cannot leave; transfer ownership first".to_string());
        }
        // Any non-owner role may leave.
    } else {
        if role_rank(caller_role) < role_rank(Role::Admin) {
            return Err("admin or owner required to revoke others".to_string());
        }
        if membership.role == Role::Owner {
            return Err("cannot revoke owner; transfer ownership first".to_string());
        }
    }

    ctx.db.account_member().id().delete(&membership.id);

    log::info!(
        "revoke_account_member account={} member={} by={}",
        account_id,
        member_id,
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
    // Humans only — apps cannot be primary or set primary.
    require_registered_user(ctx)?;

    let mut account = load_account(ctx, account_id)?;
    let caller_role = caller_role_on(ctx, account_id)?;
    if caller_role != Role::Owner {
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

        if account.user_id != Identity::ZERO && account.user_id != sender {
            // TODO: Maybe we handle this state here? Or just determine state is impossible
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
            // TODO: Should be unreachable if only Owner is ever primary.
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

/// Set or clear the account webhook URL. `None` (or blank after trim) clears.
/// Caller must be Admin+ on the account.
#[reducer]
pub fn set_account_webhook(
    ctx: &ReducerContext,
    account_id: u64,
    webhook: Option<String>,
) -> Result<(), String> {
    require_principal(ctx)?;

    let mut account = load_account(ctx, account_id)?;
    let caller_role = caller_role_on(ctx, account_id)?;
    if role_rank(caller_role) < role_rank(Role::Admin) {
        return Err("admin or owner required".to_string());
    }

    let webhook = normalize_webhook(webhook)?;
    let cleared = webhook.is_none();
    account.webhook = webhook;
    ctx.db.account().id().update(account);

    log::info!(
        "set_account_webhook account={} cleared={} by={}",
        account_id,
        cleared,
        ctx.sender()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// Prefer user over app if both somehow exist (should not happen under mutual exclusion).
fn resolve_member_kind(ctx: &ReducerContext, member_id: Identity) -> Result<MemberKind, String> {
    if ctx.db.user().id().find(&member_id).is_some() {
        return Ok(MemberKind::User);
    }
    if ctx.db.app().id().find(&member_id).is_some() {
        return Ok(MemberKind::App);
    }
    Err("member not found (not a registered user or app)".to_string())
}

fn transfer_ownership(
    ctx: &ReducerContext,
    account: &Account,
    caller_role: Role,
    grantee: &User,
    grantee_membership: Option<&AccountMember>,
) -> Result<(), String> {
    if caller_role != Role::Owner {
        return Err("only owner can grant owner".to_string());
    }

    // Primary must be cleared before ownership can move.
    if account.user_id != Identity::ZERO {
        return Err("clear primary before transferring ownership".to_string());
    }

    let caller = ctx.sender();
    let caller_membership = find_membership(ctx, account.id, caller)
        .ok_or_else(|| "caller membership missing".to_string())?;

    if caller_membership.role != Role::Owner {
        return Err("caller is not owner".to_string());
    }

    // Promote grantee to Owner (insert or update).
    upsert_membership(
        ctx,
        account.id,
        grantee.id,
        MemberKind::User,
        Role::Owner,
        grantee_membership,
    )?;

    // Demote previous Owner to Admin.
    let mut demoted = caller_membership;
    demoted.role = Role::Admin;
    demoted.updated_at = ctx.timestamp;
    ctx.db.account_member().id().update(demoted);

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
    member_id: Identity,
    kind: MemberKind,
    role: Role,
    existing: Option<&AccountMember>,
) -> Result<(), String> {
    match existing {
        Some(m) => {
            if m.role == role && m.kind == kind {
                return Ok(());
            }
            let mut updated = m.clone();
            updated.role = role;
            updated.kind = kind;
            updated.updated_at = ctx.timestamp;
            ctx.db.account_member().id().update(updated);
        }
        None => {
            ctx.db.account_member().insert(AccountMember {
                id: 0,
                account_id,
                member_id,
                kind,
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

fn caller_role_on(ctx: &ReducerContext, account_id: u64) -> Result<Role, String> {
    effective_role(ctx, account_id).ok_or_else(|| "not a member of this account".to_string())
}

fn find_membership(
    ctx: &ReducerContext,
    account_id: u64,
    member_id: Identity,
) -> Option<AccountMember> {
    ctx.db
        .account_member()
        .by_account_and_member()
        .filter((account_id, member_id))
        .next()
}

fn count_owners(ctx: &ReducerContext, account_id: u64) -> usize {
    ctx.db
        .account_member()
        .by_account_and_member()
        .filter(&account_id)
        .filter(|m| m.role == Role::Owner)
        .count()
}
