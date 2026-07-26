use crate::require_registered_user;
use crate::role_rank;
use crate::tables::*;
use spacetimedb::{ReducerContext, Table, reducer};

const MAX_IDEMPOTENCY_KEY_LEN: usize = 64;
const MAX_MEMO_LEN: usize = 32;

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
    require_registered_user(ctx)?;

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
        return Ok(());
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
    authorize_create_transfer(ctx, kind, sending.id, receiving.id, pending)?;

    let ledger_id = sending.ledger_id;
    let debit_account_id = debit_acc_id(kind, sending_account_id, receiving_account_id);
    let credit_account_id = credit_acc_id(kind, sending_account_id, receiving_account_id);

    let (mut credit_acc, mut debit_acc) = credit_debit_accounts(kind, sending, receiving);

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

    log::info!(
        "create_transfer id={} kind={:?} pending={} amount={} by={}",
        transfer.id,
        kind,
        pending,
        amount,
        ctx.sender()
    );
    Ok(())
}

#[reducer]
pub fn finalize_transfer(
    ctx: &ReducerContext,
    transfer_id: u64,
    amount: u64,
) -> Result<(), String> {
    require_registered_user(ctx)?;

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
                return Ok(());
            }
            return Err("finalize idempotency conflict".to_string());
        }
        TransferState::VoidPending => {
            if amount == 0 {
                return Ok(());
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

    authorize_finalize_transfer(ctx, &transfer)?;

    let mut debit_acc = load_account(ctx, transfer.debit_account_id)?;
    let mut credit_acc = load_account(ctx, transfer.credit_account_id)?;

    if amount == 0 {
        // Unlock full hold; historical pending_amount unchanged.
        sub_debits_pending(ctx, &mut debit_acc, held)?;
        sub_credits_pending(ctx, &mut credit_acc, held)?;

        let mut t = transfer;
        t.posted_amount = Some(0);
        t.state = TransferState::VoidPending;
        t.finalized_at = Some(ctx.timestamp);
        ctx.db.transfer().id().update(t);

        log::info!(
            "finalize_transfer id={} voided held={} by={}",
            transfer_id,
            held,
            ctx.sender()
        );
        return Ok(());
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
    ctx.db.transfer().id().update(t);

    log::info!(
        "finalize_transfer id={} posted={} refunded={} by={}",
        transfer_id,
        amount,
        refund,
        ctx.sender()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Authz
// ---------------------------------------------------------------------------

fn authorize_create_transfer(
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
            require_account_role(ctx, sending_id, UserRole::Write)
        }
        TransferKind::Liability => {
            if pending {
                return Err("liability transfers cannot be pending".to_string());
            }
            require_account_role(ctx, sending_id, UserRole::Write)
        }
        TransferKind::Redeem => {
            if pending {
                if has_account_role(ctx, sending_id, UserRole::Write)
                    || has_account_role(ctx, receiving_id, UserRole::Write)
                {
                    Ok(())
                } else {
                    Err("write access required on sender or receiver".to_string())
                }
            } else {
                require_account_role(ctx, receiving_id, UserRole::Write)
            }
        }
        TransferKind::Issue => {
            if pending {
                if has_account_role(ctx, sending_id, UserRole::Write)
                    || has_account_role(ctx, receiving_id, UserRole::Write)
                {
                    Ok(())
                } else {
                    Err("write access required on sender or receiver".to_string())
                }
            } else {
                require_account_role(ctx, sending_id, UserRole::Write)
            }
        }
    }
}

fn authorize_finalize_transfer(ctx: &ReducerContext, transfer: &Transfer) -> Result<(), String> {
    let (sending_id, receiving_id) = sender_receiver_ids(
        transfer.kind,
        transfer.credit_account_id,
        transfer.debit_account_id,
    );

    match transfer.kind {
        TransferKind::Redeem => require_account_role(ctx, receiving_id, UserRole::Write),
        TransferKind::Issue => require_account_role(ctx, sending_id, UserRole::Write),
        TransferKind::Asset | TransferKind::Liability => {
            Err("only issue/redeem pending transfers can be finalized".to_string())
        }
    }
}

fn has_account_role(ctx: &ReducerContext, account_id: u64, min: UserRole) -> bool {
    let sender = ctx.sender();
    ctx.db
        .account_user()
        .by_account_and_user()
        .filter((account_id, sender))
        .any(|p| role_rank(p.role) >= role_rank(min))
}

fn require_account_role(
    ctx: &ReducerContext,
    account_id: u64,
    min: UserRole,
) -> Result<(), String> {
    if has_account_role(ctx, account_id, min) {
        Ok(())
    } else {
        Err("insufficient account permission".to_string())
    }
}

// ---------------------------------------------------------------------------
// Kind matrix & legs
// ---------------------------------------------------------------------------

fn identify_transfer_kind(sending: AccountKind, receiving: AccountKind) -> TransferKind {
    match (sending, receiving) {
        (AccountKind::Debit, AccountKind::Debit) => TransferKind::Asset,
        (AccountKind::Credit, AccountKind::Credit) => TransferKind::Liability,
        (AccountKind::Debit, AccountKind::Credit) => TransferKind::Redeem,
        (AccountKind::Credit, AccountKind::Debit) => TransferKind::Issue,
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
