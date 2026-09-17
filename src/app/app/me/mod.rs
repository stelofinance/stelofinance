//! `GET /app/me` — You: session + owned apps.

mod apps;
mod updates;

use crate::auth::require_user;
use crate::auth::spacetimeauth::SpacetimeAuthState;
use crate::stdb::apps::fetch_my_apps;
use crate::stdb::{StdbError, acquire_user_db};
use apps::{app_forms, my_apps_list};
use topcoat::{
	Result,
	context::{Cx, app_context},
	router::{error::internal_server_error, page, query_params},
	view::view,
};

#[allow(non_snake_case)]
#[query_params]
struct MeQuery {
	appError: Option<String>,
}

#[page]
async fn me(cx: &Cx) -> Result {
	let user = require_user(cx).await?;
	let conn = acquire_user_db(cx).await?;
	let data = fetch_my_apps(conn.get())
		.await
		.map_err(|e| internal_server_error(StdbError(e)))?;
	let mint_enabled = app_context::<SpacetimeAuthState>(cx).configured();
	let app_error = query_params::<MeQuery>(cx)
		.ok()
		.and_then(|q| q.appError.clone())
		.map(|c| match c.as_str() {
			"oauth" => "SpacetimeAuth sign-in failed.".to_owned(),
			"ticket" => "Couldn't create that app (name taken or not allowed).".to_owned(),
			"name" => "Enter an app name (max 64 characters).".to_owned(),
			"config" => "SpacetimeAuth isn't configured.".to_owned(),
			_ => c,
		})
		.unwrap_or_default();
	let signals = format!(
		"{{appName:'',appError:{},appIdToken:'',appRefreshToken:'',appMintName:'',appMintId:'',appCopied:'',creatingApp:false,replacingApp:false}}",
		js_single(&app_error)
	);

	view! {
		<main
			id="page-content"
			class="mx-auto flex w-full max-w-3xl flex-col px-3 py-6 text-white sm:px-5 md:px-8 md:py-10"
			data-signals=(signals)
			data-init="@get('/app/me/apps/mint'); @get('/app/me/updates')"
		>
			<h1 class="mb-6 text-2xl font-medium md:text-3xl">"You"</h1>
			<article class="rounded-lg border border-neutral-800 bg-neutral-950 px-3 py-3 sm:px-4 sm:py-4">
				<p class="truncate text-xl font-medium">(user.bitcraft_username.clone())</p>
				<p class="mt-1 text-sm text-neutral-300">"Signed in with BitAuth"</p>
				if user.is_admin {
					<a
						href="/app/admin"
						class="mt-3 inline-flex text-sm text-anakiwa hover:text-white"
					>
						"Admin"
					</a>
				}
			</article>
			my_apps_list(data: data, mint_enabled: mint_enabled)
			app_forms(mint_enabled: mint_enabled)
			<p
				class="mt-2 text-sm text-red-400"
				data-show="$appError"
				data-text="$appError"
			></p>
			<section class="mt-6 rounded-lg border border-neutral-800 bg-neutral-950 px-3 py-3 sm:px-4 sm:py-4">
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
	}
}

fn js_single(s: &str) -> String {
	let mut out = String::from("'");
	for c in s.chars() {
		match c {
			'\\' => out.push_str("\\\\"),
			'\'' => out.push_str("\\'"),
			'\n' => out.push_str("\\n"),
			c => out.push(c),
		}
	}
	out.push('\'');
	out
}
