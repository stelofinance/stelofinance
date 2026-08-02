use crate::tables::*;
use spacetimedb::{AnonymousViewContext, Identity, SpacetimeType, Timestamp, ViewContext, view};

// ---------------------------------------------------------------------------
// Row types
// ---------------------------------------------------------------------------

#[derive(SpacetimeType, Clone, Debug)]
pub struct MyUserRow {
    pub id: Identity,
    pub bitcraft_username: String,
    pub created_at: Timestamp,
    pub is_admin: bool,
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct MyAccountRow {
    pub account_id: u64,
    pub address: String,
    pub kind: AccountKind,
    /// Available balance (posted − opposing posted/pending), by account kind.
    pub balance: u64,
    pub ledger_id: u64,
    pub ledger_name: String,
    pub ledger_scale: u8,
    pub ledger_kind: LedgerKind,
    pub role: Role,
    pub is_primary: bool,
    /// Username of the user with `Role::Owner` on this account (not the primary flag).
    pub owner_username: Option<String>,
    /// Present only when caller's role is Admin or Owner.
    pub webhook: Option<String>,
    pub created_at: Timestamp,
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct MyAccountMemberRow {
    /// `account_member` row id (stable PK for client cache).
    pub id: u64,
    pub account_id: u64,
    pub member_id: Identity,
    pub kind: MemberKind,
    /// Username (User) or app name (App).
    pub name: String,
    pub role: Role,
    pub updated_at: Timestamp,
    pub created_at: Timestamp,
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct MyTransferRow {
    pub id: u64,
    pub debit_account_id: u64,
    pub credit_account_id: u64,
    pub debit_address: String,
    pub credit_address: String,
    pub debit_username: Option<String>,
    pub credit_username: Option<String>,
    pub pending_amount: Option<u64>,
    pub posted_amount: Option<u64>,
    pub ledger_id: u64,
    pub ledger_name: String,
    pub ledger_scale: u8,
    pub kind: TransferKind,
    pub state: TransferState,
    pub memo: Option<String>,
    pub created_at: Timestamp,
    pub finalized_at: Option<Timestamp>,
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct AccountDirectoryRow {
    pub account_id: u64,
    pub address: String,
    pub ledger_id: u64,
    pub primary_username: Option<String>,
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct LedgerAuditRow {
    pub ledger_id: u64,
    pub ledger_name: String,
    /// Σ available balances of debit-normal (Debit kind) accounts.
    pub debits_net: i64,
    /// Σ available balances of credit-normal (Credit kind) accounts.
    pub credits_net: i64,
    /// `debits_net == credits_net` (double-entry conservation in available-balance terms).
    pub balanced: bool,
}

/// Metadata for HTTP API tokens (never includes the secret).
#[derive(SpacetimeType, Clone, Debug)]
pub struct MyAccountTokenRow {
    pub id: u64,
    pub account_id: u64,
    pub label: String,
    pub created_by: Identity,
    pub created_at: Timestamp,
}

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

#[view(accessor = my_user, public)]
fn my_user(ctx: &ViewContext) -> Option<MyUserRow> {
    ctx.db.user().id().find(&ctx.sender()).map(|u| MyUserRow {
        id: u.id,
        bitcraft_username: u.bitcraft_username,
        created_at: u.created_at,
        is_admin: u.is_admin,
    })
}

#[view(accessor = my_accounts, public, primary_key = account_id)]
fn my_accounts(ctx: &ViewContext) -> Vec<MyAccountRow> {
    let sender = ctx.sender();
    let mut out = Vec::new();

    for (account_id, role) in memberships_for_sender(ctx) {
        let Some(acc) = ctx.db.account().id().find(&account_id) else {
            continue;
        };
        let Some(ledger) = ctx.db.ledger().id().find(&acc.ledger_id) else {
            continue;
        };

        let is_primary = acc.user_id == sender;
        let owner_username = owner_username_for_account(ctx, acc.id);
        let webhook = match role {
            Role::Admin | Role::Owner => acc.webhook.clone(),
            _ => None,
        };

        out.push(MyAccountRow {
            account_id: acc.id,
            address: acc.address.clone(),
            kind: acc.kind,
            balance: computed_balance(&acc),
            ledger_id: ledger.id,
            ledger_name: ledger.name.clone(),
            ledger_scale: ledger.scale,
            ledger_kind: ledger.kind,
            role,
            is_primary,
            owner_username,
            webhook,
            created_at: acc.created_at,
        });
    }

    out
}

/// Users and apps on every account the caller can access (Read+).
/// Filter with SQL for one account: `WHERE account_id = …`
#[view(accessor = my_accounts_members, public, primary_key = id)]
fn my_accounts_members(ctx: &ViewContext) -> Vec<MyAccountMemberRow> {
    let mut out = Vec::new();
    let mut seen_accounts = Vec::new();

    for (account_id, _) in memberships_for_sender(ctx) {
        if seen_accounts.contains(&account_id) {
            continue;
        }
        seen_accounts.push(account_id);

        for member in ctx
            .db
            .account_member()
            .by_account_and_member()
            .filter(&account_id)
        {
            let name = member_display_name(&ctx.db, member.member_id, member.kind);
            out.push(MyAccountMemberRow {
                id: member.id,
                account_id: member.account_id,
                member_id: member.member_id,
                kind: member.kind,
                name,
                role: member.role,
                updated_at: member.updated_at,
                created_at: member.created_at,
            });
        }
    }

    out
}

/// Transfers involving any of the caller's accounts.
///
/// No `primary_key` yet: if the caller has ACL on both legs we may emit the same
/// transfer twice (product choice). Declaring `primary_key = id` would reject that.
#[view(accessor = my_transfers, public)]
fn my_transfers(ctx: &ViewContext) -> Vec<MyTransferRow> {
    let mut out = Vec::new();
    let mut seen_accounts = Vec::new();

    for (account_id, _) in memberships_for_sender(ctx) {
        if seen_accounts.contains(&account_id) {
            continue;
        }
        seen_accounts.push(account_id);

        for tr in ctx.db.transfer().debit_account_id().filter(&account_id) {
            if let Some(row) = enrich_transfer(ctx, &tr) {
                out.push(row);
            }
        }
        for tr in ctx.db.transfer().credit_account_id().filter(&account_id) {
            if let Some(row) = enrich_transfer(ctx, &tr) {
                out.push(row);
            }
        }
    }

    out
}

/// Public recipient / search catalog. Anonymous callers allowed.
/// Edge applies LIKE-style term filtering client-side or via SQL on this view.
#[view(accessor = account_directory, public, primary_key = account_id)]
fn account_directory(ctx: &AnonymousViewContext) -> Vec<AccountDirectoryRow> {
    // Full scan via ranged index on ledger_id (views may not use table `.iter()`).
    ctx.db
        .account()
        .ledger_id()
        .filter(0u64..)
        .map(|acc| AccountDirectoryRow {
            account_id: acc.id,
            address: acc.address,
            ledger_id: acc.ledger_id,
            primary_username: username_for(&ctx.db, acc.user_id),
        })
        .collect()
}

/// App-admin ledger conservation check. Non-admins get an empty result.
#[view(accessor = ledger_audit, public, primary_key = ledger_id)]
fn ledger_audit(ctx: &ViewContext) -> Vec<LedgerAuditRow> {
    let Some(user) = ctx.db.user().id().find(&ctx.sender()) else {
        return Vec::new();
    };
    if !user.is_admin {
        return Vec::new();
    }

    // ledger_id → (name, debits_net, credits_net)
    let mut by_ledger: Vec<(u64, String, i64, i64)> = Vec::new();

    for acc in ctx.db.account().ledger_id().filter(0u64..) {
        let bal = computed_balance_i64(&acc);
        let entry = by_ledger
            .iter_mut()
            .find(|(id, _, _, _)| *id == acc.ledger_id);
        match entry {
            Some((_, _, debits_net, credits_net)) => match acc.kind {
                AccountKind::Debit => *debits_net = debits_net.saturating_add(bal),
                AccountKind::Credit => *credits_net = credits_net.saturating_add(bal),
            },
            None => {
                let name = ctx
                    .db
                    .ledger()
                    .id()
                    .find(&acc.ledger_id)
                    .map(|l| l.name)
                    .unwrap_or_default();
                let (d, c) = match acc.kind {
                    AccountKind::Debit => (bal, 0i64),
                    AccountKind::Credit => (0i64, bal),
                };
                by_ledger.push((acc.ledger_id, name, d, c));
            }
        }
    }

    by_ledger
        .into_iter()
        .map(
            |(ledger_id, ledger_name, debits_net, credits_net)| LedgerAuditRow {
                ledger_id,
                ledger_name,
                debits_net,
                credits_net,
                balanced: debits_net == credits_net,
            },
        )
        .collect()
}

/// Tokens on accounts where the caller is Admin or Owner.
/// Secret is never included.
#[view(accessor = my_accounts_tokens, public, primary_key = id)]
fn my_accounts_tokens(ctx: &ViewContext) -> Vec<MyAccountTokenRow> {
    let mut out = Vec::new();

    for (account_id, role) in memberships_for_sender(ctx) {
        if crate::role_rank(role) < crate::role_rank(Role::Admin) {
            continue;
        }
        for tok in ctx.db.account_token().account_id().filter(&account_id) {
            out.push(MyAccountTokenRow {
                id: tok.id,
                account_id: tok.account_id,
                label: tok.label,
                created_by: tok.created_by,
                created_at: tok.created_at,
            });
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Accounts where the caller has a role via `account_member`.
fn memberships_for_sender(ctx: &ViewContext) -> Vec<(u64, Role)> {
    let sender = ctx.sender();
    ctx.db
        .account_member()
        .member_id()
        .filter(&sender)
        .map(|m| (m.account_id, m.role))
        .collect()
}

/// Username of the first `Role::Owner` membership on this account.
fn owner_username_for_account(ctx: &ViewContext, account_id: u64) -> Option<String> {
    for member in ctx
        .db
        .account_member()
        .by_account_and_member()
        .filter(&account_id)
    {
        if member.role == Role::Owner {
            return username_for(&ctx.db, member.member_id);
        }
    }
    None
}

fn member_display_name(
    db: &spacetimedb::LocalReadOnly,
    member_id: Identity,
    kind: MemberKind,
) -> String {
    match kind {
        MemberKind::User => db
            .user()
            .id()
            .find(&member_id)
            .map(|u| u.bitcraft_username)
            .unwrap_or_default(),
        MemberKind::App => db
            .app()
            .id()
            .find(&member_id)
            .map(|a| a.name)
            .unwrap_or_default(),
    }
}

/// Available balance matching legacy app semantics (never negative → u64).
fn computed_balance(acc: &Account) -> u64 {
    computed_balance_i64(acc).max(0) as u64
}

fn computed_balance_i64(acc: &Account) -> i64 {
    match acc.kind {
        AccountKind::Debit => {
            let posted = acc.debits_posted as i64;
            let oppose = (acc.credits_posted as i64).saturating_add(acc.credits_pending as i64);
            posted.saturating_sub(oppose)
        }
        AccountKind::Credit => {
            let posted = acc.credits_posted as i64;
            let oppose = (acc.debits_posted as i64).saturating_add(acc.debits_pending as i64);
            posted.saturating_sub(oppose)
        }
    }
}

fn username_for(db: &spacetimedb::LocalReadOnly, user_id: Identity) -> Option<String> {
    if user_id == Identity::ZERO {
        return None;
    }
    db.user().id().find(&user_id).map(|u| u.bitcraft_username)
}

fn enrich_transfer(ctx: &ViewContext, tr: &Transfer) -> Option<MyTransferRow> {
    let debit = ctx.db.account().id().find(&tr.debit_account_id)?;
    let credit = ctx.db.account().id().find(&tr.credit_account_id)?;
    let ledger = ctx.db.ledger().id().find(&tr.ledger_id)?;

    Some(MyTransferRow {
        id: tr.id,
        debit_account_id: tr.debit_account_id,
        credit_account_id: tr.credit_account_id,
        debit_address: debit.address,
        credit_address: credit.address,
        debit_username: username_for(&ctx.db, debit.user_id),
        credit_username: username_for(&ctx.db, credit.user_id),
        pending_amount: tr.pending_amount,
        posted_amount: tr.posted_amount,
        ledger_id: ledger.id,
        ledger_name: ledger.name,
        ledger_scale: ledger.scale,
        kind: tr.kind,
        state: tr.state,
        memo: tr.memo.clone(),
        created_at: tr.created_at,
        finalized_at: tr.finalized_at,
    })
}
