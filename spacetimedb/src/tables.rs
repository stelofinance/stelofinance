use spacetimedb::{Identity, SpacetimeType, Timestamp, table};

#[table(accessor = user)]
pub struct User {
    #[primary_key]
    #[auto_inc]
    id: u64, // Maybe just Identity as primary key?

    /// SpacetimeDB client identity for this user. Lookup via `ctx.sender()`.
    #[unique]
    owner: Identity,

    #[unique]
    bitcraft_username: String,

    #[unique]
    bitcraft_id: String, // Might not be needed

    created_at: Timestamp,
}

#[derive(SpacetimeType)]
pub enum LedgerKind {
    Digital,
    Derivation,
    Physical,
}

#[spacetimedb::table(accessor = ledger, public)]
pub struct Ledger {
    #[primary_key]
    #[auto_inc]
    id: u64,

    #[unique]
    name: String,

    asset_scale: u8,
    kind: LedgerKind,
}

#[derive(SpacetimeType)]
pub enum AccountKind {
    Credit,
    Debit,
}

#[spacetimedb::table(accessor = account)]
pub struct Account {
    #[primary_key]
    #[auto_inc]
    id: u64,

    // Group / payment address within a ledger.
    #[index(btree)]
    address: String,

    webhook: Option<String>,

    /// Primary owner user id, if any. Indexed with ledger via `by_user_ledger`.
    #[index(btree)]
    user_id: Option<u64>, // Could this be to the Identity instead?

    debits_pending: u64,
    debits_posted: u64,
    credits_pending: u64,
    credits_posted: u64,

    #[index(btree)]
    ledger_id: u64,
    kind: AccountKind,
    created_at: Timestamp,
}

#[derive(SpacetimeType)]
pub enum UserRole {
    Read,
    Write,
    Admin,
    Owner,
}

#[spacetimedb::table(accessor = account_permission)]
pub struct AccountUser {
    #[primary_key]
    #[auto_inc]
    id: u64,
    #[index(btree)]
    account_id: u64,
    #[index(btree)]
    user_id: u64,
    role: UserRole,
    updated_at: Timestamp,
    created_at: Timestamp,
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
