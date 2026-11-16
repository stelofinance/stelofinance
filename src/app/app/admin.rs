//! `GET /app/admin` — platform-admin reducer forms. Start: `create_ledger`.

use crate::auth::require_user;
use crate::auth::user::AppUser;
use crate::domain::MAX_LEDGER_SCALE;
use crate::module_bindings::LedgerKind;
use crate::stdb::{acquire_user_db, create_user_ledger};
use topcoat::{
	Result,
	context::Cx,
	metadata::Metadata,
	router::{error::not_found, page},
	runtime::{Event, procedure, signal},
	view::{View, component, view},
};

async fn require_admin(cx: &Cx) -> Result<&AppUser> {
	let user = require_user(cx).await?;
	if !user.is_admin {
		return Err(not_found().into());
	}
	Ok(user)
}

/// `GET /app/admin` — 404 unless `User.is_admin`.
#[page(with(Metadata::new().title("Admin · Stelo Finance")))]
async fn page(cx: &Cx) -> Result<impl View> {
	let _user = require_admin(cx).await?;
	Ok(view! {
    <main
        id="page-content"
        class="mx-auto flex w-full max-w-3xl flex-col px-3 py-6 text-white sm:px-5 md:px-8 md:py-10"
    >
        <h1 class="mb-2 text-2xl font-medium md:text-3xl">"Admin"</h1>
        <p class="mb-6 text-sm text-neutral-400">
            "Forms that call admin-only reducers."
        </p>
        create_ledger_form()
    </main>
})
}

#[component]
async fn create_ledger_form(cx: &Cx) -> Result<impl View> {
	let name = signal(cx, String::new);
	let scale = signal(cx, || "0".to_owned());
	let kind = signal(cx, || "Physical".to_owned());
	let error = signal(cx, String::new);
	let ok = signal(cx, String::new);
	let busy = signal(cx, || false);

	Ok(view! {
    <section class="rounded-lg border border-neutral-800 bg-neutral-950 p-4 sm:p-5">
        <h2 class="text-lg font-medium">"Create ledger"</h2>
        <p class="mt-1 text-sm text-neutral-300">
            "Adds a public asset to the catalog ("
            <code class="text-neutral-200">"create_ledger"</code>
            ")."
        </p>

        <label class="mt-4 block">
            <span class="text-sm text-neutral-300">"Name"</span>
            <input
                type="text"
                class="mt-1 w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm text-white placeholder:text-neutral-400"
                :value=$(name.get())
                @input=$(|e: Event| name.set(e.target.value))
                placeholder="Hexcoin"
                autocomplete="off"
            >
        </label>

        <label class="mt-4 block">
            <span class="text-sm text-neutral-300">"Scale "</span>
            <span class="text-sm text-neutral-400">"(decimal places)"</span>
            <input
                type="text"
                inputmode="numeric"
                class="mt-1 w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm text-white placeholder:text-neutral-400"
                :value=$(scale.get())
                @input=$(|e: Event| scale.set(e.target.value))
                placeholder="0"
                autocomplete="off"
            >
        </label>

        <label class="mt-4 block">
            <span class="text-sm text-neutral-300">"Kind"</span>
            <select
                class="mt-1 w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm"
                :value=$(kind.get())
                @change=$(|e: Event| kind.set(e.target.value))
            >
                <option value="Physical">"In-game item"</option>
                <option value="Digital">"Digital"</option>
                <option value="Derivation">"Derivation"</option>
            </select>
        </label>

        <button
            type="button"
            class="mt-5 w-full cursor-pointer rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600 disabled:cursor-not-allowed disabled:opacity-50"
            :disabled=$(if busy.get() { true } else { name.get().trim() == "" })
            @click=$(async |_e| {
                busy.set(true);
                error.set("".to_owned());
                ok.set("".to_owned());
                let result = create_ledger(name.get(), scale.get(), kind.get()).await;
                busy.set(false);
                if result.is_ok() {
                    ok.set(result.unwrap());
                    name.set("".to_owned());
                } else {
                    error.set(result.unwrap_err());
                }
            })
        >
            "Create ledger"
        </button>
        <p class="mt-3 text-sm text-red-400" :hidden=$(error.get().is_empty())>
            $(error.get())
        </p>
        <p class="mt-3 text-sm text-anakiwa" :hidden=$(ok.get().is_empty())>
            $(ok.get())
        </p>
    </section>
})
}

/// `create_ledger`. Procedures skip the page guard, so admin is re-checked.
/// `Ok(Ok(msg))` is the success text.
#[procedure("/app/admin/ledgers")]
async fn create_ledger(
	cx: &Cx,
	name: String,
	scale: String,
	kind: String,
) -> Result<std::result::Result<String, String>> {
	if require_admin(cx).await.is_err() {
		return Ok(Err("Admins only.".into()));
	}
	let name = name.trim().to_owned();
	if name.is_empty() {
		return Ok(Err("Ledger name required.".into()));
	}
	let scale = match scale.trim().parse::<u8>() {
		Ok(scale) if scale <= MAX_LEDGER_SCALE => scale,
		_ => return Ok(Err(format!("Scale must be 0–{MAX_LEDGER_SCALE}."))),
	};
	let kind = match kind.trim() {
		"Digital" => LedgerKind::Digital,
		"Derivation" => LedgerKind::Derivation,
		"Physical" => LedgerKind::Physical,
		_ => return Ok(Err("Pick a ledger kind.".into())),
	};

	let Ok(conn) = acquire_user_db(cx).await else {
		return Ok(Err("Your session expired. Reload the page.".into()));
	};
	match create_user_ledger(conn.get(), name.clone(), scale, kind).await {
		Ok(()) => Ok(Ok(format!("Created {name}."))),
		Err(e) => Ok(Err(e.user_message().to_owned())),
	}
}
