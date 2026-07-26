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

#[spacetimedb::table(
    accessor = account_user,
    index(accessor = by_account_and_user, btree(columns = [account_id, user_id]))
)]
#[derive(Clone, Debug)]
pub struct AccountUser {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub account_id: u64,
    #[index(btree)]
    pub user_id: Identity,
    pub role: UserRole,
    pub updated_at: Timestamp,
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


