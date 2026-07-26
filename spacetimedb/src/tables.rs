use spacetimedb::{Identity, SpacetimeType, Timestamp, table};

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

    pub asset_scale: u8,
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

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserRole {
    Read,
    Write,
    Admin,
    Owner,
}

#[spacetimedb::table(accessor = account_permission)]
#[derive(Clone, Debug)]
pub struct AccountUser {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    #[index(btree)]
    pub account_id: u64,
    #[index(btree)]
    pub user_id: Identity,
    pub role: UserRole,
    pub updated_at: Timestamp,
    pub created_at: Timestamp,
}

#[derive(SpacetimeType)]
pub enum TransferState {
    Posted,
    Pending,
    PostPending, // Can also be partially posted if posted_amount < pending_amount
    VoidPending, // Completely void the transfer, all pending funds unlocked
}
#[derive(SpacetimeType)]
pub enum TransferKind {
    Liability, // Credit <-> Credit
    Asset,     // Debit <-> Debit
    Issue,     // Credit -> Debit
    Redeem,    // Debit -> Credit
}

#[spacetimedb::table(accessor = transfer)]
pub struct Transfer {
    #[primary_key]
    #[auto_inc]
    id: u64,

    #[index(btree)]
    debit_account_id: u64,
    #[index(btree)]
    credit_account_id: u64,

    pending_amount: Option<u64>,
    posted_amount: Option<u64>,

    #[index(btree)]
    ledger_id: u64,

    kind: TransferKind,
    memo: Option<String>,
    created_at: Timestamp,
    finalized_at: Option<Timestamp>,
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
