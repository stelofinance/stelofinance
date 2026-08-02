use crate::has_minimum_role;
use crate::require_account_role;
use crate::require_principal;
use crate::tables::*;
use crate::webhooks::enqueue_transfer_webhooks;
use spacetimedb::{ReducerContext, Table, reducer};

const MAX_IDEMPOTENCY_KEY_LEN: usize = 64;
const MAX_MEMO_LEN: usize = 32;

/// Who is authorizing a transfer mutation.
#[derive(Clone, Copy, Debug)]
pub(crate) enum TransferActor {
    /// BitAuth user or app: use `account_member` roles via `ctx.sender()`.
    Identity,
    /// HTTP account token: full ops authority for this account id only.
    TokenAccount(u64),
}

/// Result of create, including whether this was an idempotent replay.
// TODO: Do we really need the enum? Could maybe just return one value better
#[derive(Clone, Debug)]
pub(crate) struct CreateTransferOutcome {
    pub transfer: Transfer,
    /// `true` if the same idempotency key + hash was replayed (HTTP → 200 vs 201).
    pub replay: bool,
}

#[reducer]
pub fn create_transfer(
    ctx: &ReducerContext,
    sending_account_id: u64,
    receiving_account_id: u64,
    amount: u64,
    memo: Option<String>,
    idempotency_key: String,
    pending: bool,
) -> Result<(), String> {
    require_principal(ctx)?;
    create_transfer_core(
        ctx,
        TransferActor::Identity,
        sending_account_id,
        receiving_account_id,
        amount,
        memo,
        idempotency_key,
        pending,
    )?;
    Ok(())
}

#[reducer]
pub fn finalize_transfer(
    ctx: &ReducerContext,
    transfer_id: u64,
    amount: u64,
) -> Result<(), String> {
    require_principal(ctx)?;
    finalize_transfer_core(ctx, TransferActor::Identity, transfer_id, amount)?;
    Ok(())
}

/// Shared create path for reducers and HTTP handlers.
pub(crate) fn create_transfer_core(
    ctx: &ReducerContext,
    actor: TransferActor,
    sending_account_id: u64,
    receiving_account_id: u64,
    amount: u64,
    memo: Option<String>,
    idempotency_key: String,
    pending: bool,
) -> Result<CreateTransferOutcome, String> {
    let key = idempotency_key.trim().to_string();
    if key.is_empty() {
        return Err("idempotency key required".to_string());
    }
    if key.len() > MAX_IDEMPOTENCY_KEY_LEN {
        return Err("idempotency key invalid".to_string());
    }
    if amount < 1 {
        return Err("invalid quantity".to_string());
    }
    if sending_account_id == receiving_account_id {
        return Err("sender is receiver".to_string());
    }

    // TODO: Just guard all non-senders for now, update later
    if let TransferActor::TokenAccount(token_acc) = actor {
        if token_acc != sending_account_id {
            return Err("token account must be the sending account".to_string());
        }
    }

    let memo = normalize_memo(memo)?;
    let req_hash = transfer_request_hash(
        sending_account_id,
        receiving_account_id,
        amount,
        &memo,
        pending,
    );

    if let Some(existing) = find_idempotency(ctx, sending_account_id, &key) {
        if existing.request_hash != req_hash {
            return Err("idempotency key conflict".to_string());
        }
        let transfer = ctx
            .db
            .transfer()
            .id()
            .find(&existing.transfer_id)
            .ok_or_else(|| "idempotent transfer missing".to_string())?;
        return Ok(CreateTransferOutcome {
            transfer,
            replay: true,
        });
    }

    let sending = ctx
        .db
        .account()
        .id()
        .find(&sending_account_id)
        .ok_or_else(|| "sending account not found".to_string())?;
    let receiving = ctx
        .db
        .account()
        .id()
        .find(&receiving_account_id)
        .ok_or_else(|| "receiving account not found".to_string())?;

    if sending.ledger_id != receiving.ledger_id {
        return Err("incompatible account ledgers".to_string());
    }

    let kind = identify_transfer_kind(sending.kind, receiving.kind);
    authorize_create_transfer(ctx, actor, kind, sending.id, receiving.id, pending)?;

    let ledger_id = sending.ledger_id;
    let debit_account_id = debit_acc_id(kind, sending_account_id, receiving_account_id);
    let credit_account_id = credit_acc_id(kind, sending_account_id, receiving_account_id);

    let (mut credit_acc, mut debit_acc) =
        credit_debit_accounts(kind, sending.clone(), receiving.clone());

    if pending {
        add_debits_pending(ctx, &mut debit_acc, amount)?;
        add_credits_pending(ctx, &mut credit_acc, amount)?;
    } else {
        add_debits_posted(ctx, &mut debit_acc, amount)?;
        add_credits_posted(ctx, &mut credit_acc, amount)?;
    }

    let transfer = ctx.db.transfer().insert(Transfer {
        id: 0,
        debit_account_id,
        credit_account_id,
        pending_amount: if pending { Some(amount) } else { None },
        posted_amount: if pending { None } else { Some(amount) },
        ledger_id,
        kind,
        state: if pending {
            TransferState::Pending
        } else {
            TransferState::Posted
        },
        memo: memo.clone(),
        created_at: ctx.timestamp,
        finalized_at: if pending { None } else { Some(ctx.timestamp) },
    });

    ctx.db.transfer_idempotency().insert(TransferIdempotency {
        id: 0,
        account_id: sending_account_id,
        key,
        transfer_id: transfer.id,
        request_hash: req_hash,
        created_at: ctx.timestamp,
    });

    enqueue_transfer_webhooks(ctx, &transfer, &sending, &receiving);

    log::info!(
        "create_transfer id={} kind={:?} pending={} amount={} actor={:?}",
        transfer.id,
        kind,
        pending,
        amount,
        actor
    );
    Ok(CreateTransferOutcome {
        transfer,
        replay: false,
    })
}

/// Shared finalize path for reducers and HTTP handlers.
pub(crate) fn finalize_transfer_core(
    ctx: &ReducerContext,
    actor: TransferActor,
    transfer_id: u64,
    amount: u64,
) -> Result<Transfer, String> {
    let transfer = ctx
        .db
        .transfer()
        .id()
        .find(&transfer_id)
        .ok_or_else(|| "transfer not found".to_string())?;

    match transfer.state {
        TransferState::Pending => {}
        TransferState::PostPending => {
            let posted = transfer.posted_amount.unwrap_or(0);
            if amount >= 1 && amount == posted {
                return Ok(transfer);
            }
            return Err("finalize idempotency conflict".to_string());
        }
        TransferState::VoidPending => {
            if amount == 0 {
                return Ok(transfer);
            }
            return Err("finalize idempotency conflict".to_string());
        }
        TransferState::Posted => {
            return Err("transfer is not pending".to_string());
        }
    }

    let held = transfer.pending_amount.unwrap_or(0);
    if held == 0 {
        return Err("no pending funds to finalize".to_string());
    }

    authorize_finalize_transfer(ctx, actor, &transfer)?;

    // TODO: Optimize fetching these twice (here and just below)
    let mut debit_acc = load_account(ctx, transfer.debit_account_id)?;
    let mut credit_acc = load_account(ctx, transfer.credit_account_id)?;

    // Snapshot accounts before balance updates for webhook URL capture.
    let (sending_id, receiving_id) = sender_receiver_ids(
        transfer.kind,
        transfer.credit_account_id,
        transfer.debit_account_id,
    );
    let sending_snap = load_account(ctx, sending_id)?;
    let receiving_snap = load_account(ctx, receiving_id)?;

    if amount == 0 {
        // Unlock full hold; historical pending_amount unchanged.
        sub_debits_pending(ctx, &mut debit_acc, held)?;
        sub_credits_pending(ctx, &mut credit_acc, held)?;

        let mut t = transfer;
        t.posted_amount = Some(0);
        t.state = TransferState::VoidPending;
        t.finalized_at = Some(ctx.timestamp);
        ctx.db.transfer().id().update(t.clone());

        enqueue_transfer_webhooks(ctx, &t, &sending_snap, &receiving_snap);

        log::info!(
            "finalize_transfer id={} voided held={} actor={:?}",
            transfer_id,
            held,
            actor
        );
        return Ok(t);
    }

    if amount > held {
        return Err("post amount exceeds pending amount".to_string());
    }

    let refund = held - amount;
    move_debits_pending_to_posted(ctx, &mut debit_acc, amount)?;
    move_credits_pending_to_posted(ctx, &mut credit_acc, amount)?;
    if refund > 0 {
        sub_debits_pending(ctx, &mut debit_acc, refund)?;
        sub_credits_pending(ctx, &mut credit_acc, refund)?;
    }

    let mut t = transfer;
    // pending_amount stays as historical hold size.
    t.posted_amount = Some(amount);
    t.state = TransferState::PostPending;
    t.finalized_at = Some(ctx.timestamp);
    ctx.db.transfer().id().update(t.clone());

    enqueue_transfer_webhooks(ctx, &t, &sending_snap, &receiving_snap);

    log::info!(
        "finalize_transfer id={} posted={} refunded={} actor={:?}",
        transfer_id,
        amount,
        refund,
        actor
    );
    Ok(t)
}

// ---------------------------------------------------------------------------
// Authz
// ---------------------------------------------------------------------------

// TODO: Clean this up
fn authorize_create_transfer(
    ctx: &ReducerContext,
    actor: TransferActor,
    kind: TransferKind,
    sending_id: u64,
    receiving_id: u64,
    pending: bool,
) -> Result<(), String> {
    match actor {
        TransferActor::Identity => {
            authorize_create_identity(ctx, kind, sending_id, receiving_id, pending)
        }
        TransferActor::TokenAccount(token_acc) => {
            // Token is always the sender; create_transfer_core already checks that.
            // Token implies full operational authority on the token account.
            // For posted redeem, Identity path requires Write on *receiving*;
            // HTTP API still allows the token-holder as sender only (legacy shape).
            // Reject cases where Identity would require a different account.
            match kind {
                TransferKind::Asset | TransferKind::Liability => {
                    if pending {
                        return Err(if matches!(kind, TransferKind::Asset) {
                            "asset transfers cannot be pending".to_string()
                        } else {
                            "liability transfers cannot be pending".to_string()
                        });
                    }
                    if token_acc != sending_id {
                        return Err("insufficient account permission".to_string());
                    }
                    Ok(())
                }
                TransferKind::Issue => {
                    // Posted issue: Write on sending. Pending: Write on send or recv.
                    if pending {
                        if token_acc == sending_id || token_acc == receiving_id {
                            Ok(())
                        } else {
                            Err("insufficient account permission".to_string())
                        }
                    } else if token_acc == sending_id {
                        Ok(())
                    } else {
                        Err("insufficient account permission".to_string())
                    }
                }
                TransferKind::Redeem => {
                    // Posted redeem: Identity needs Write on *receiving*.
                    // Token is always sender (debit) — allow only pending redeem
                    // where sender Write is enough, or if token is on receiving.
                    if pending {
                        if token_acc == sending_id || token_acc == receiving_id {
                            Ok(())
                        } else {
                            Err("insufficient account permission".to_string())
                        }
                    } else if token_acc == receiving_id {
                        Ok(())
                    } else {
                        // Token on debit cannot post redeem (needs credit/receiving Write).
                        Err("posted redeem requires token on receiving account".to_string())
                    }
                }
            }
        }
    }
}

fn authorize_create_identity(
    ctx: &ReducerContext,
    kind: TransferKind,
    sending_id: u64,
    receiving_id: u64,
    pending: bool,
) -> Result<(), String> {
    match kind {
        TransferKind::Asset => {
            if pending {
                return Err("asset transfers cannot be pending".to_string());
            }
            require_account_role(ctx, sending_id, Role::Write)
        }
        TransferKind::Liability => {
            if pending {
                return Err("liability transfers cannot be pending".to_string());
            }
            require_account_role(ctx, sending_id, Role::Write)
        }
        TransferKind::Redeem => {
            if pending {
                if has_minimum_role(ctx, sending_id, Role::Write)
                    || has_minimum_role(ctx, receiving_id, Role::Write)
                {
                    Ok(())
                } else {
                    Err("write access required on sender or receiver".to_string())
                }
            } else {
                require_account_role(ctx, receiving_id, Role::Write)
            }
        }
        TransferKind::Issue => {
            if pending {
                if has_minimum_role(ctx, sending_id, Role::Write)
                    || has_minimum_role(ctx, receiving_id, Role::Write)
                {
                    Ok(())
                } else {
                    Err("write access required on sender or receiver".to_string())
                }
            } else {
                require_account_role(ctx, sending_id, Role::Write)
            }
        }
    }
}

// TODO: Clean this up
fn authorize_finalize_transfer(
    ctx: &ReducerContext,
    actor: TransferActor,
    transfer: &Transfer,
) -> Result<(), String> {
    let (sending_id, receiving_id) = sender_receiver_ids(
        transfer.kind,
        transfer.credit_account_id,
        transfer.debit_account_id,
    );

    match actor {
        TransferActor::Identity => match transfer.kind {
            TransferKind::Redeem => require_account_role(ctx, receiving_id, Role::Write),
            TransferKind::Issue => require_account_role(ctx, sending_id, Role::Write),
            TransferKind::Asset | TransferKind::Liability => {
                Err("only issue/redeem pending transfers can be finalized".to_string())
            }
        },
        TransferActor::TokenAccount(token_acc) => match transfer.kind {
            TransferKind::Redeem => {
                if token_acc == receiving_id {
                    Ok(())
                } else {
                    Err("insufficient account permission".to_string())
                }
            }
            TransferKind::Issue => {
                if token_acc == sending_id {
                    Ok(())
                } else {
                    Err("insufficient account permission".to_string())
                }
            }
            TransferKind::Asset | TransferKind::Liability => {
                Err("only issue/redeem pending transfers can be finalized".to_string())
            }
        },
    }
}

// ---------------------------------------------------------------------------
// Kind matrix & legs
// ---------------------------------------------------------------------------

fn identify_transfer_kind(sending: AccountKind, receiving: AccountKind) -> TransferKind {
    match (sending, receiving) {
        (AccountKind::Credit, AccountKind::Credit) => TransferKind::Liability,
        (AccountKind::Debit, AccountKind::Debit) => TransferKind::Asset,
        (AccountKind::Credit, AccountKind::Debit) => TransferKind::Issue,
        (AccountKind::Debit, AccountKind::Credit) => TransferKind::Redeem,
    }
}

/// Double-entry legs as account rows: `(credit_leg, debit_leg)`.
fn credit_debit_accounts(
    kind: TransferKind,
    sending: Account,
    receiving: Account,
) -> (Account, Account) {
    match kind {
        TransferKind::Liability => (receiving, sending),
        TransferKind::Asset | TransferKind::Issue | TransferKind::Redeem => (sending, receiving),
    }
}

fn credit_acc_id(kind: TransferKind, sending_id: u64, receiving_id: u64) -> u64 {
    match kind {
        TransferKind::Liability => receiving_id,
        TransferKind::Asset | TransferKind::Issue | TransferKind::Redeem => sending_id,
    }
}

fn debit_acc_id(kind: TransferKind, sending_id: u64, receiving_id: u64) -> u64 {
    match kind {
        TransferKind::Liability => sending_id,
        TransferKind::Asset | TransferKind::Issue | TransferKind::Redeem => receiving_id,
    }
}

fn sender_receiver_ids(
    kind: TransferKind,
    credit_account_id: u64,
    debit_account_id: u64,
) -> (u64, u64) {
    match kind {
        TransferKind::Liability => (debit_account_id, credit_account_id),
        TransferKind::Asset | TransferKind::Issue | TransferKind::Redeem => {
            (credit_account_id, debit_account_id)
        }
    }
}

// ---------------------------------------------------------------------------
// Balance mutations — mutate already-loaded rows in place, then persist
// ---------------------------------------------------------------------------

fn credit_can_add_debits(acc: &Account, amount: u64) -> Result<(), String> {
    if acc.kind != AccountKind::Credit {
        return Ok(());
    }
    let used = acc
        .debits_pending
        .checked_add(acc.debits_posted)
        .and_then(|v| v.checked_add(amount))
        .ok_or_else(|| "balance overflow".to_string())?;
    if acc.credits_posted < used {
        return Err("invalid balance".to_string());
    }
    Ok(())
}

fn debit_can_add_credits(acc: &Account, amount: u64) -> Result<(), String> {
    if acc.kind != AccountKind::Debit {
        return Ok(());
    }
    let used = acc
        .credits_pending
        .checked_add(acc.credits_posted)
        .and_then(|v| v.checked_add(amount))
        .ok_or_else(|| "balance overflow".to_string())?;
    if acc.debits_posted < used {
        return Err("invalid balance".to_string());
    }
    Ok(())
}

fn add_debits_posted(ctx: &ReducerContext, acc: &mut Account, amount: u64) -> Result<(), String> {
    credit_can_add_debits(acc, amount)?;
    acc.debits_posted = acc
        .debits_posted
        .checked_add(amount)
        .ok_or_else(|| "balance overflow".to_string())?;
    ctx.db.account().id().update(acc.clone());
    Ok(())
}

fn add_credits_posted(ctx: &ReducerContext, acc: &mut Account, amount: u64) -> Result<(), String> {
    debit_can_add_credits(acc, amount)?;
    acc.credits_posted = acc
        .credits_posted
        .checked_add(amount)
        .ok_or_else(|| "balance overflow".to_string())?;
    ctx.db.account().id().update(acc.clone());
    Ok(())
}

fn add_debits_pending(ctx: &ReducerContext, acc: &mut Account, amount: u64) -> Result<(), String> {
    credit_can_add_debits(acc, amount)?;
    acc.debits_pending = acc
        .debits_pending
        .checked_add(amount)
        .ok_or_else(|| "balance overflow".to_string())?;
    ctx.db.account().id().update(acc.clone());
    Ok(())
}

fn add_credits_pending(ctx: &ReducerContext, acc: &mut Account, amount: u64) -> Result<(), String> {
    debit_can_add_credits(acc, amount)?;
    acc.credits_pending = acc
        .credits_pending
        .checked_add(amount)
        .ok_or_else(|| "balance overflow".to_string())?;
    ctx.db.account().id().update(acc.clone());
    Ok(())
}

fn sub_debits_pending(ctx: &ReducerContext, acc: &mut Account, amount: u64) -> Result<(), String> {
    if acc.debits_pending < amount {
        return Err("invalid balance".to_string());
    }
    acc.debits_pending -= amount;
    ctx.db.account().id().update(acc.clone());
    Ok(())
}

fn sub_credits_pending(ctx: &ReducerContext, acc: &mut Account, amount: u64) -> Result<(), String> {
    if acc.credits_pending < amount {
        return Err("invalid balance".to_string());
    }
    acc.credits_pending -= amount;
    ctx.db.account().id().update(acc.clone());
    Ok(())
}

fn move_debits_pending_to_posted(
    ctx: &ReducerContext,
    acc: &mut Account,
    amount: u64,
) -> Result<(), String> {
    if acc.debits_pending < amount {
        return Err("invalid balance".to_string());
    }
    acc.debits_pending -= amount;
    acc.debits_posted = acc
        .debits_posted
        .checked_add(amount)
        .ok_or_else(|| "balance overflow".to_string())?;
    ctx.db.account().id().update(acc.clone());
    Ok(())
}

fn move_credits_pending_to_posted(
    ctx: &ReducerContext,
    acc: &mut Account,
    amount: u64,
) -> Result<(), String> {
    if acc.credits_pending < amount {
        return Err("invalid balance".to_string());
    }
    acc.credits_pending -= amount;
    acc.credits_posted = acc
        .credits_posted
        .checked_add(amount)
        .ok_or_else(|| "balance overflow".to_string())?;
    ctx.db.account().id().update(acc.clone());
    Ok(())
}

fn load_account(ctx: &ReducerContext, account_id: u64) -> Result<Account, String> {
    ctx.db
        .account()
        .id()
        .find(&account_id)
        .ok_or_else(|| format!("account {account_id} not found"))
}

// ---------------------------------------------------------------------------
// Idempotency helpers
// ---------------------------------------------------------------------------

fn normalize_memo(memo: Option<String>) -> Result<Option<String>, String> {
    let Some(raw) = memo else {
        return Ok(None);
    };
    let m = raw.trim();
    if m.is_empty() {
        return Ok(None);
    }
    if m.len() > MAX_MEMO_LEN {
        return Err("memo exceeds length limit".to_string());
    }
    Ok(Some(m.to_string()))
}

fn transfer_request_hash(
    sending_id: u64,
    receiving_id: u64,
    amount: u64,
    memo: &Option<String>,
    pending: bool,
) -> String {
    let m = memo.as_deref().unwrap_or("");
    format!("{sending_id}|{receiving_id}|{amount}|{m}|{pending}")
}

fn find_idempotency(
    ctx: &ReducerContext,
    account_id: u64,
    key: &str,
) -> Option<TransferIdempotency> {
    ctx.db
        .transfer_idempotency()
        .by_account_and_key()
        .filter((account_id, key))
        .next()
}
