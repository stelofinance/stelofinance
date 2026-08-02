use spacetimedb::{Identity, ScheduleAt, SpacetimeType, Timestamp, table};

use crate::apps::*;

// DB Config Table, for meta stuff
#[table(accessor = config)]
pub struct Config {
    #[primary_key]
    pub owner: Identity,
}

#[table(accessor = user)]
#[derive(Clone, Debug)]
pub struct User {
    #[primary_key]
    pub id: Identity,

    #[unique]
    pub bitcraft_username: String,

    pub created_at: Timestamp,
    #[default(false)]
    pub is_admin: bool,
}

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerKind {
    Digital,
    Derivation,
    Physical,
}

#[spacetimedb::table(accessor = ledger, public)]
#[derive(Clone, Debug)]
pub struct Ledger {
    #[primary_key]
    #[auto_inc]
    pub id: u64,

    #[unique]
    pub name: String,

    pub scale: u8,
    pub kind: LedgerKind,
}

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountKind {
    Credit,
    Debit,
}

#[spacetimedb::table(
    accessor = account,
    index(accessor = by_user_and_ledger, btree(columns = [user_id, ledger_id]))
)]
#[derive(Clone, Debug)]
pub struct Account {
    #[primary_key]
    #[auto_inc]
    pub id: u64,

    // Group / payment address within a ledger.
    #[index(btree)]
    pub address: String,

    pub webhook: Option<String>,

    /// Primary owner for this ledger, if any.
    /// `Identity::ZERO` is our way of doing `Option<Identity>` while keeping an index.
    pub user_id: Identity,

    pub debits_pending: u64,
    pub debits_posted: u64,
    pub credits_pending: u64,
    pub credits_posted: u64,

    #[index(btree)]
    pub ledger_id: u64,
    pub kind: AccountKind,
    pub created_at: Timestamp,
}

/// Shared role ladder for account members (users and apps).
/// Ordering: Read < Write < Admin < Owner. Apps may hold at most Admin.
#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Read,
    Write,
    Admin,
    Owner,
}

/// Whether a membership row is a human user or an app principal.
#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberKind {
    User,
    App,
}

/// Unified account ACL for users and apps.
#[spacetimedb::table(
    accessor = account_member,
    index(accessor = by_account_and_member, btree(columns = [account_id, member_id]))
)]
#[derive(Clone, Debug)]
pub struct AccountMember {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub account_id: u64,
    #[index(btree)]
    pub member_id: Identity,
    pub kind: MemberKind,
    pub role: Role,
    pub updated_at: Timestamp,
    pub created_at: Timestamp,
}

/// HTTP API tokens for programmatic access to an account.
#[spacetimedb::table(accessor = account_token)]
#[derive(Clone, Debug)]
pub struct AccountToken {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    #[index(btree)]
    pub account_id: u64,
    #[unique]
    pub token: String,
    pub label: String,
    pub created_by: Identity,
    pub created_at: Timestamp,
}

/// Third-party / bot principal (SpacetimeAuth anonymous Identity).
/// Bound on first connect when an `app_ticket` matches the JWT `sub`.
#[spacetimedb::table(accessor = app)]
#[derive(Clone, Debug)]
pub struct App {
    #[primary_key]
    pub id: Identity,

    #[unique]
    pub name: String,

    /// Human user who created the app (rename/delete / replace identity later)
    pub created_by: Identity,
    pub updated_at: Timestamp,
    pub created_at: Timestamp,
}

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppTicketPurpose {
    Create,  // First time registration
    Replace, // Rebind Identity on App
}

/// Pending app create/replace; expires via schedule (~15 minutes)
#[table(accessor = app_ticket, scheduled(expire_app_ticket, at = expires_at))]
#[derive(Clone, Debug)]
pub struct AppTicket {
    #[primary_key]
    #[auto_inc]
    pub id: u64,

    pub expires_at: ScheduleAt, // Fire TTL cleanup

    pub created_by: Identity,

    /// Create: desired app name (must be free).
    /// Replace: existing app name
    #[unique]
    pub name: String,

    /// OIDC `sub` that will own this app Identity on connect
    #[unique]
    pub sub: String,

    pub purpose: AppTicketPurpose,
    pub created_at: Timestamp,
}

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferState {
    Posted,
    Pending,
    PostPending,
    VoidPending,
}

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferKind {
    Liability, // Credit <-> Credit
    Asset,     // Debit <-> Debit
    Issue,     // Credit -> Debit
    Redeem,    // Debit -> Credit
}

#[spacetimedb::table(accessor = transfer)]
#[derive(Clone, Debug)]
pub struct Transfer {
    #[primary_key]
    #[auto_inc]
    pub id: u64,

    #[index(btree)]
    pub debit_account_id: u64,
    #[index(btree)]
    pub credit_account_id: u64,

    pub pending_amount: Option<u64>,
    pub posted_amount: Option<u64>,

    #[index(btree)]
    pub ledger_id: u64,

    pub kind: TransferKind,
    pub state: TransferState,
    pub memo: Option<String>,
    pub created_at: Timestamp,
    pub finalized_at: Option<Timestamp>,
}

#[spacetimedb::table(
    accessor = transfer_idempotency,
    index(accessor = by_account_and_key, btree(columns = [account_id, key])))]
#[derive(Clone, Debug)]
pub struct TransferIdempotency {
    #[primary_key]
    #[auto_inc]
    pub id: u64,

    pub account_id: u64,
    pub key: String,
    pub transfer_id: u64,
    pub request_hash: String,
    pub created_at: Timestamp,
}
