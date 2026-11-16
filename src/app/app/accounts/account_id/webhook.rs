//! Set / clear account webhook URL (Admin+).

use crate::stdb::account::set_webhook;
use crate::stdb::acquire_user_db;
use topcoat::{
	Result,
	context::Cx,
	runtime::{Event, procedure, record, signal},
	view::{View, component, view},
};

/// Lives below API tokens, outside the live region so the input keeps typing.
#[component]
pub async fn webhook_form(cx: &Cx, account_id: u64, initial_url: String) -> Result<impl View> {
	let url = signal(cx, || initial_url.clone());
	let notice = signal(cx, String::new);
	let error = signal(cx, String::new);
	let busy = signal(cx, || false);
	Ok(view! {
    <section class="mt-10">
        <h2 class="text-sm font-medium uppercase tracking-wide text-neutral-400">
            "Webhook"
        </h2>
        <p class="mt-2 text-sm text-neutral-400">
            "Stelo POSTs JSON here when this account sends or receives a transfer. Absolute http(s) URL."
        </p>
        <div class="mt-3 flex flex-col gap-2 sm:flex-row">
            <input
                type="url"
                class="w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm placeholder:text-neutral-400"
                :value=$(url.get())
                @input=$(|e: Event| {
                    url.set(e.target.value);
                    notice.set("".to_owned());
                })
                placeholder="https://example.com/hooks/stelo"
                autocomplete="off"
            >
            <button
                type="button"
                class="cursor-pointer rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600 disabled:cursor-not-allowed disabled:opacity-50"
                :disabled=$(busy.get())
                @click=$(async |_e| {
                    notice.set("".to_owned());
                    error.set("".to_owned());
                    busy.set(true);
                    let result = save_webhook(account_id, url.get()).await;
                    busy.set(false);
                    if result.is_ok() {
                        let saved = result.unwrap();
                        url.set(saved.url);
                        notice.set(saved.notice);
                    } else {
                        error.set(result.unwrap_err());
                    }
                })
            >
                "Save"
            </button>
            <button
                type="button"
                class="cursor-pointer rounded-md border border-neutral-700 px-4 py-2 text-sm text-neutral-300 hover:border-neutral-500 hover:text-white disabled:cursor-not-allowed disabled:opacity-50"
                :hidden=$(url.get().is_empty())
                :disabled=$(busy.get())
                @click=$(async |_e| {
                    url.set("".to_owned());
                    notice.set("".to_owned());
                    error.set("".to_owned());
                    busy.set(true);
                    let result = save_webhook(account_id, "".to_owned()).await;
                    busy.set(false);
                    if result.is_ok() {
                        let saved = result.unwrap();
                        url.set(saved.url);
                        notice.set(saved.notice);
                    } else {
                        error.set(result.unwrap_err());
                    }
                })
            >
                "Clear"
            </button>
        </div>
        <p class="mt-3 text-sm text-anakiwa" :hidden=$(notice.get().is_empty())>
            $(notice.get())
        </p>
        <p class="mt-3 text-sm text-red-400" :hidden=$(error.get().is_empty())>
            $(error.get())
        </p>
    </section>
})
}

/// What a save stored (`url` is empty when cleared) and the message to show.
#[record]
#[derive(Clone, Debug)]
struct WebhookSaved {
	url: String,
	notice: String,
}

/// Set or clear (blank) the webhook. The module validates the URL.
#[procedure("/app/accounts/webhook")]
async fn save_webhook(
	cx: &Cx,
	account_id: u64,
	url: String,
) -> Result<std::result::Result<WebhookSaved, String>> {
	let trimmed = url.trim();
	let webhook = if trimmed.is_empty() {
		None
	} else {
		Some(trimmed.to_owned())
	};
	let Ok(conn) = acquire_user_db(cx).await else {
		return Ok(Err("Your session expired. Reload the page.".into()));
	};
	Ok(set_webhook(conn.get(), account_id, webhook.clone())
		.await
		.map(|()| {
			let notice = if webhook.is_none() {
				"Webhook cleared."
			} else {
				"Saved."
			};
			WebhookSaved {
				url: webhook.unwrap_or_default(),
				notice: notice.to_owned(),
			}
		})
		.map_err(|e| e.user_message().to_owned()))
}
