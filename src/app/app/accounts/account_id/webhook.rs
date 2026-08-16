//! Set / clear account webhook URL (Admin+).

use super::AccountId;
use crate::auth::require_user;
use crate::stdb::account::set_webhook;
use crate::stdb::acquire_user_db;
use serde::{Deserialize, Serialize};
use topcoat::{
	Result,
	context::Cx,
	datastar::{PatchSignals, Signals},
	router::{path_param, route},
	view::{component, view},
};

/// Lives below API tokens. Input is signal-bound so remorphs of the token list stay out of the way.
#[component]
pub async fn webhook_form(account_id: u64) -> Result {
	let save = format!(
		"$webhookNotice = ''; $webhookError = ''; @post('/app/accounts/{account_id}/webhook')"
	);
	let clear = format!(
		"$webhookUrl = ''; $webhookNotice = ''; $webhookError = ''; @post('/app/accounts/{account_id}/webhook')"
	);
	view! {
		<section class="mt-10">
			<h2 class="text-sm font-medium uppercase tracking-wide text-neutral-400">
				"Webhook"
			</h2>
			<p class="mt-2 text-sm text-neutral-400">
				"Stelo POSTs JSON here when this account sends or receives a transfer. Absolute http(s) URL. Deliveries are at least once — use the transfer id as an idempotency key."
			</p>
			<div class="mt-3 flex flex-col gap-2 sm:flex-row">
				<input
					type="url"
					class="w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm placeholder:text-neutral-400"
					data-bind="webhookUrl"
					placeholder="https://example.com/hooks/stelo"
					autocomplete="off"
					data-on:input="$webhookNotice = ''"
				>
				<button
					type="button"
					class="cursor-pointer rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600"
					data-on:click=(save)
					data-indicator="savingWebhook"
				>
					"Save"
				</button>
				<button
					type="button"
					class="cursor-pointer rounded-md border border-neutral-700 px-4 py-2 text-sm text-neutral-300 hover:border-neutral-500 hover:text-white"
					data-on:click=(clear)
					data-show="$webhookUrl"
					data-indicator="savingWebhook"
					style="display: none"
				>
					"Clear"
				</button>
			</div>
			<p
				class="mt-3 text-sm text-anakiwa"
				data-show="$webhookNotice"
				data-text="$webhookNotice"
			></p>
			<p
				class="mt-3 text-sm text-red-400"
				data-show="$webhookError"
				data-text="$webhookError"
			></p>
		</section>
	}
}

/// `POST /app/accounts/{account_id}/webhook` — set or clear (`None` if blank).
#[route(POST)]
async fn save_webhook(cx: &Cx, Signals(form): Signals<WebhookSignals>) -> Result<PatchSignals> {
	let _user = require_user(cx).await?;
	let account_id = *path_param::<AccountId>(cx)?;
	let trimmed = form.webhook_url.trim();
	let webhook = if trimmed.is_empty() {
		None
	} else {
		Some(trimmed.to_owned())
	};
	let conn = acquire_user_db(cx).await?;
	match set_webhook(conn.get(), account_id, webhook.clone()).await {
		Ok(()) => {
			let cleared = webhook.is_none();
			PatchSignals::json(&WebhookPatch {
				webhook_error: String::new(),
				webhook_url: webhook.unwrap_or_default(),
				webhook_notice: if cleared {
					"Webhook cleared.".to_owned()
				} else {
					"Saved.".to_owned()
				},
			})
		}
		Err(e) => PatchSignals::json(&WebhookErrorPatch {
			webhook_error: e,
			webhook_notice: String::new(),
		}),
	}
}

#[derive(Debug, Deserialize)]
struct WebhookSignals {
	#[serde(default, rename = "webhookUrl")]
	webhook_url: String,
}

#[derive(Serialize)]
struct WebhookPatch {
	#[serde(rename = "webhookError")]
	webhook_error: String,
	#[serde(rename = "webhookUrl")]
	webhook_url: String,
	#[serde(rename = "webhookNotice")]
	webhook_notice: String,
}

#[derive(Serialize)]
struct WebhookErrorPatch {
	#[serde(rename = "webhookError")]
	webhook_error: String,
	#[serde(rename = "webhookNotice")]
	webhook_notice: String,
}
