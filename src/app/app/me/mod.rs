//! `GET /app/me` — You: session + owned apps.

mod apps;

use crate::auth::oidc::OidcAuth;
use crate::auth::require_user;
use apps::{app_forms, apps_list};
use topcoat::{
	Result,
	context::{Cx, context},
	metadata::Metadata,
	router::page,
	runtime::signal,
	view::{View, component, view},
};

#[page(with(Metadata::new().title("You · Stelo Finance")))]
async fn me(cx: &Cx) -> Result<impl View> {
	let user = require_user(cx).await?;
	let signed_in = match context::<OidcAuth>(cx).provider_name() {
		Some(name) => format!("Signed in with {name}"),
		None => "Signed in".to_owned(),
	};

	Ok(view! {
    <main
        id="page-content"
        class="mx-auto flex w-full max-w-3xl flex-col px-3 py-6 text-white sm:px-5 md:px-8 md:py-10"
    >
        <h1 class="mb-6 text-2xl font-medium md:text-3xl">"You"</h1>
        <article
            class="rounded-lg border border-neutral-800 bg-neutral-950 px-3 py-3 sm:px-4 sm:py-4"
        >
            <p class="truncate text-xl font-medium">(user.bitcraft_username.clone())</p>
            <p class="mt-1 text-sm text-neutral-300">(signed_in)</p>
            if user.is_admin {
                <a
                    href="/app/admin"
                    class="mt-3 inline-flex text-sm text-anakiwa hover:text-white"
                >
                    "Admin"
                </a>
            }
        </article>
        apps_panel()
        <section
            class="mt-6 rounded-lg border border-neutral-800 bg-neutral-950 px-3 py-3 sm:px-4 sm:py-4"
        >
            <h2 class="text-sm font-medium uppercase tracking-wide text-neutral-400">
                "Session"
            </h2>
            <p class="mt-2 text-sm text-neutral-300">"Signs you out of Stelo."</p>
            <form method="post" action="/logout" class="mt-4">
                <button
                    type="submit"
                    class="cursor-pointer rounded-md border border-neutral-600 px-4 py-2 text-sm font-medium text-neutral-200 hover:border-neutral-400 hover:text-white"
                >
                    "Log out"
                </button>
            </form>
        </section>
    </main>
})
}

/// Owns the apps signals; wires the live list, forms and error line.
#[component]
async fn apps_panel(cx: &Cx) -> Result<impl View> {
	let name = signal(cx, String::new);
	let error = signal(cx, String::new);
	let token = signal(cx, String::new);
	let mint_name = signal(cx, String::new);
	let mint_id = signal(cx, String::new);
	let minting = signal(cx, || false);
	let creating = signal(cx, || false);
	let replacing = signal(cx, || false);

	Ok(view! {
    apps_list(
        creating: creating.clone(),
        replacing: replacing.clone(),
        mint_id: mint_id.clone(),
        name: name.clone(),
        error: error.clone()
    )
    app_forms(
        creating: creating.clone(),
        replacing: replacing.clone(),
        token: token.clone(),
        mint_name: mint_name.clone(),
        mint_id: mint_id.clone(),
        name: name.clone(),
        error: error.clone(),
        minting: minting.clone()
    )
    <p class="mt-2 text-sm text-red-400" :hidden=$(error.get().is_empty())>
        $(error.get())
    </p>
})
}
