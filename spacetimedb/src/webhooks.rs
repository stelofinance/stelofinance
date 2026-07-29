use crate::tables::*;
use spacetimedb::{
    ProcedureContext, ReducerContext, ScheduleAt, Table, TimeDuration, Timestamp, procedure, table,
};
use std::time::Duration;

const USER_AGENT: &str = "Stelo-Webhooks/1.0";
const MAX_ATTEMPTS: u32 = 10;
const HTTP_TIMEOUT: Duration = Duration::from_secs(5);

#[table(accessor = webhook_delivery, scheduled(deliver_webhook))]
#[derive(Clone, Debug)]
pub struct WebhookDelivery {
    #[primary_key]
    #[auto_inc]
    pub id: u64,

    pub scheduled_at: ScheduleAt,
    pub account_id: u64,
    pub transfer_id: u64,
    pub url: String,
    pub payload_json: String,
    pub attempts: u32,
}

/// Enqueue webhook deliveries for sending and/or receiving accounts that have a URL set.
/// Call inside the same reducer transaction as the transfer mutation.
pub(crate) fn enqueue_transfer_webhooks(
    ctx: &ReducerContext,
    transfer: &Transfer,
    sending: &Account,
    receiving: &Account,
) {
    let Ok(payload_json) = build_payload_json(transfer) else {
        log::error!(
            "webhook enqueue: failed to build payload for transfer {}",
            transfer.id
        );
        return;
    };

    if let Some(url) = sending.webhook.as_ref() {
        insert_delivery(ctx, sending.id, transfer.id, url, &payload_json, 0);
    }
    if let Some(url) = receiving.webhook.as_ref() {
        insert_delivery(ctx, receiving.id, transfer.id, url, &payload_json, 0);
    }
}

// TODO: Remove this function, it's really not needed
fn insert_delivery(
    ctx: &ReducerContext,
    account_id: u64,
    transfer_id: u64,
    url: &str,
    payload_json: &str,
    attempts: u32,
) {
    insert_delivery_at(
        ctx,
        account_id,
        transfer_id,
        url,
        payload_json,
        attempts,
        ctx.timestamp,
    );
}

fn insert_delivery_at(
    ctx: &impl DeliveryInserter,
    account_id: u64,
    transfer_id: u64,
    url: &str,
    payload_json: &str,
    attempts: u32,
    when: Timestamp,
) {
    ctx.insert_webhook_delivery(WebhookDelivery {
        id: 0,
        scheduled_at: ScheduleAt::Time(when),
        account_id,
        transfer_id,
        url: url.to_string(),
        payload_json: payload_json.to_string(),
        attempts,
    });
    log::info!(
        "webhook scheduled transfer={} account={} attempts={}",
        transfer_id,
        account_id,
        attempts
    );
}

/// Abstraction so reducers and procedure `with_tx` contexts can both insert jobs.
trait DeliveryInserter {
    fn insert_webhook_delivery(&self, row: WebhookDelivery);
}

impl DeliveryInserter for ReducerContext {
    fn insert_webhook_delivery(&self, row: WebhookDelivery) {
        self.db.webhook_delivery().insert(row);
    }
}

impl DeliveryInserter for spacetimedb::TxContext {
    fn insert_webhook_delivery(&self, row: WebhookDelivery) {
        self.db.webhook_delivery().insert(row);
    }
}

fn build_payload_json(transfer: &Transfer) -> Result<String, String> {
    let amount = match transfer.state {
        TransferState::Pending => transfer.pending_amount.unwrap_or(0),
        TransferState::Posted | TransferState::PostPending | TransferState::VoidPending => {
            transfer.posted_amount.unwrap_or(0)
        }
    };

    let kind = match transfer.kind {
        TransferKind::Liability => "Liability",
        TransferKind::Asset => "Asset",
        TransferKind::Issue => "Issue",
        TransferKind::Redeem => "Redeem",
    };
    let state = match transfer.state {
        TransferState::Posted => "Posted",
        TransferState::Pending => "Pending",
        TransferState::PostPending => "PostPending",
        TransferState::VoidPending => "VoidPending",
    };

    let created_at = transfer
        .created_at
        .to_rfc3339()
        .unwrap_or_else(|_| format!("{}", transfer.created_at));

    let body = serde_json::json!({
        "id": transfer.id,
        "debitAccId": transfer.debit_account_id,
        "creditAccId": transfer.credit_account_id,
        "amount": amount,
        "ledgerId": transfer.ledger_id,
        "kind": kind,
        "state": state,
        "memo": transfer.memo,
        "createdAt": created_at,
    });

    serde_json::to_string(&body).map_err(|e| e.to_string())
}

fn backoff_duration(attempt: u32) -> Duration {
    // attempt is the *next* attempt number after a failure (1-based for first retry).
    // Delays aligned with the legacy JetStream worker.
    let delays_secs: &[u64] = &[
        1,    // after attempt 0 fails → try 1
        5,    // 2
        30,   // 3
        120,  // 4
        300,  // 5
        900,  // 6
        1800, // 7
        3600, // 8
        7200, // 9+
    ];
    let idx = attempt.saturating_sub(1) as usize;
    let secs = delays_secs
        .get(idx)
        .copied()
        .unwrap_or(*delays_secs.last().unwrap());
    Duration::from_secs(secs)
}

#[procedure]
pub fn deliver_webhook(ctx: &mut ProcedureContext, job: WebhookDelivery) {
    if ctx.sender() != ctx.database_identity() {
        log::warn!(
            "deliver_webhook rejected: non-scheduler caller {}",
            ctx.sender()
        );
        return;
    }

    let request = match spacetimedb::http::Request::builder()
        .uri(&job.url)
        .method("POST")
        .header("Content-Type", "application/json")
        .header("User-Agent", USER_AGENT)
        .extension(spacetimedb::http::Timeout(TimeDuration::from(HTTP_TIMEOUT)))
        .body(job.payload_json.clone())
    {
        Ok(r) => r,
        Err(e) => {
            log::error!(
                "deliver_webhook: bad request transfer={} account={}: {e}",
                job.transfer_id,
                job.account_id
            );
            requeue_or_drop(ctx, &job, &format!("build request: {e}"));
            return;
        }
    };

    match ctx.http.send(request) {
        Ok(response) => {
            let status = response.status();
            if status.is_success() {
                log::info!(
                    "deliver_webhook ok transfer={} account={} status={} attempts={}",
                    job.transfer_id,
                    job.account_id,
                    status.as_u16(),
                    job.attempts
                );
            } else {
                let code = status.as_u16();
                log::warn!(
                    "deliver_webhook non-2xx transfer={} account={} status={} attempts={}",
                    job.transfer_id,
                    job.account_id,
                    code,
                    job.attempts
                );
                requeue_or_drop(ctx, &job, &format!("http status {code}"));
            }
        }
        Err(e) => {
            log::warn!(
                "deliver_webhook transport error transfer={} account={} attempts={}: {e}",
                job.transfer_id,
                job.account_id,
                job.attempts
            );
            requeue_or_drop(ctx, &job, &format!("transport: {e}"));
        }
    }
}

fn requeue_or_drop(ctx: &mut ProcedureContext, job: &WebhookDelivery, reason: &str) {
    let next_attempts = job.attempts.saturating_add(1);
    if next_attempts >= MAX_ATTEMPTS {
        log::error!(
            "deliver_webhook exhausted transfer={} account={} attempts={} reason={}",
            job.transfer_id,
            job.account_id,
            next_attempts,
            reason
        );
        return;
    }

    let delay = backoff_duration(next_attempts);
    let when = ctx.timestamp + delay;
    let account_id = job.account_id;
    let transfer_id = job.transfer_id;
    let url = job.url.clone();
    let payload_json = job.payload_json.clone();

    ctx.with_tx(|tx| {
        insert_delivery_at(
            tx,
            account_id,
            transfer_id,
            &url,
            &payload_json,
            next_attempts,
            when,
        );
    });
}
