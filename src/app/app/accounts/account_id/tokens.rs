//! Create API tokens on an account (Admin+). Revoking lives in the live
//! token list (`live.rs`).

use crate::stdb::account::create_account_api_token;
use crate::stdb::acquire_user_db;
use crate::ui::runtime::copy_button;
use topcoat::{
	Result,
	context::Cx,
	runtime::{Event, Signal, procedure, signal},
	view::{View, component, view},
};

/// Create form + one-time secret sheet. Outside the live region so the form
/// and the secret survive live updates. The secret exists only in `new_token`,
/// which never enters a shard (signal values are sent with shard requests);
/// the live region reads `has_token` instead.
#[component]
pub async fn token_forms(
	cx: &Cx,
	account_id: u64,
	creating_token: Signal<bool>,
	new_token: Signal<String>,
	has_token: Signal<bool>,
	token_label: Signal<String>,
	token_error: Signal<String>,
) -> Result<impl View> {
	let busy = signal(cx, || false);
	Ok(view! {
    <section
        class="mt-4 rounded-lg border border-neutral-800 bg-neutral-950 p-4 sm:p-5"
        :hidden=$(if creating_token.get() { new_token.get() != "" } else { true })
    >
        <h2 class="text-lg font-medium">"New token"</h2>
        <p class="mt-1 text-sm text-neutral-300">
            "The secret is shown once. Copy it then, it won't be shown again."
        </p>
        <input
            type="text"
            class="mt-4 w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm placeholder:text-neutral-400"
            :value=$(token_label.get())
            @input=$(|e: Event| token_label.set(e.target.value))
            maxlength="32"
            placeholder="Label (optional)"
            autocomplete="off"
        >
        <p class="mt-3 text-sm text-red-400" :hidden=$(token_error.get().is_empty())>
            $(token_error.get())
        </p>
        <div class="mt-4 flex flex-wrap gap-2">
            <button
                type="button"
                class="cursor-pointer rounded-md border border-neutral-700 px-3 py-1.5 text-sm text-neutral-300 hover:border-neutral-500 hover:text-white"
                @click=$(|_e| {
                    creating_token.set(false);
                    token_label.set("".to_owned());
                    token_error.set("".to_owned());
                })
            >
                "Cancel"
            </button>
            <button
                type="button"
                class="cursor-pointer rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600 disabled:cursor-not-allowed disabled:opacity-50"
                :disabled=$(busy.get())
                @click=$(async |_e| {
                    busy.set(true);
                    let result = create_token(account_id, token_label.get()).await;
                    busy.set(false);
                    if result.is_ok() {
                        token_error.set("".to_owned());
                        new_token.set(result.unwrap());
                        has_token.set(true);
                        creating_token.set(false);
                        token_label.set("".to_owned());
                    } else {
                        token_error.set(result.unwrap_err());
                    }
                })
            >
                "Create"
            </button>
        </div>
    </section>
    <section
        class="mt-4 rounded-lg border border-neutral-800 bg-neutral-950 p-4 sm:p-5"
        :hidden=$(new_token.get() == "")
    >
        <h2 class="text-lg font-medium">"Token created"</h2>
        <p class="mt-1 text-sm text-neutral-300">
            "Copy it now, this is the only time you’ll see it."
        </p>
        <div
            class="mt-4 flex items-stretch overflow-hidden rounded-lg border border-neutral-800"
        >
            <code
                class="min-w-0 flex-1 truncate bg-neutral-900 px-3 py-2 text-sm text-anakiwa"
            >
                $(new_token.get())
            </code>
            copy_button(
                text: $(new_token.get()),
                class: "shrink-0 cursor-pointer border-l border-neutral-800 px-3 text-xs text-neutral-300 hover:bg-neutral-900 hover:text-white"
            )
        </div>
        <button
            type="button"
            class="mt-4 cursor-pointer rounded-md bg-neutral-800 px-4 py-2 text-sm hover:bg-neutral-700"
            @click=$(|_e| {
                new_token.set("".to_owned());
                has_token.set(false);
                creating_token.set(false);
                token_label.set("".to_owned());
                token_error.set("".to_owned());
            })
        >
            "Done"
        </button>
    </section>
})
}

/// Mint a token and return its secret (shown once, held only in a signal).
#[procedure("/app/accounts/tokens/create")]
async fn create_token(
	cx: &Cx,
	account_id: u64,
	label: String,
) -> Result<std::result::Result<String, String>> {
	let Ok(conn) = acquire_user_db(cx).await else {
		return Ok(Err("Your session expired. Reload the page.".into()));
	};
	// The input's `maxlength` is client-side only.
	let label = label.trim();
	if label.chars().count() > 32 {
		return Ok(Err("Labels are at most 32 characters.".into()));
	}
	Ok(
		create_account_api_token(conn.get(), account_id, label.to_owned())
			.await
			.map_err(|e| e.user_message().to_owned()),
	)
}
