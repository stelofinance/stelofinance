//! `GET /app/me` — You. H5: Stelo-only logout. More profile later.

use crate::auth::require_user;
use topcoat::{Result, context::Cx, router::page, view::view};

/// `GET /app/me` — identity + session. Account-age / wallet stats later.
#[page]
async fn me(cx: &Cx) -> Result {
	let user = require_user(cx).await?;

	view! {
		<main
			id="page-content"
			class="mx-auto flex w-full max-w-3xl flex-col px-3 py-6 text-white sm:px-5 md:px-8 md:py-10"
		>
			<h1 class="mb-6 text-2xl font-medium md:text-3xl">"You"</h1>
			<article class="rounded-lg border border-neutral-800 bg-neutral-950 px-3 py-3 sm:px-4 sm:py-4">
				<p class="truncate text-xl font-medium">(user.bitcraft_username.clone())</p>
				<p class="mt-1 text-sm text-neutral-300">"Signed in with BitAuth"</p>
			</article>
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
