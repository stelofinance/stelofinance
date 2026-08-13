use crate::auth::require_user;
use topcoat::{Result, context::Cx, router::page, view::view};

/// `GET /app/me` — profile shell.
///
/// Planned (not yet): logout control, account age, wallet membership stats,
/// and other user-level settings. Logout route exists at `/auth/bitauth/logout`.
#[page]
async fn me(cx: &Cx) -> Result {
	let user = require_user(cx).await?;

	view! {
		<main class="mx-auto max-w-lg px-4 py-10 text-white">
			<h1 class="text-2xl font-medium md:text-3xl">"You"</h1>
			<p class="mt-2 text-neutral-300">
				"Signed in as "
				<span class="text-white">(user.bitcraft_username.clone())</span>
			</p>

			<section class="mt-8 rounded-lg border border-neutral-800 bg-neutral-950 p-5">
				<h2 class="text-sm font-medium uppercase tracking-wide text-neutral-400">
					"Coming soon"
				</h2>
				<ul class="mt-3 list-inside list-disc space-y-1 text-sm text-neutral-300">
					<li>"Logout"</li>
					<li>"How long you've been on Stelo"</li>
					<li>"How many accounts you're on"</li>
					<li>"More profile & session controls"</li>
				</ul>
			</section>

			// Temporary until the profile page is real — still link logout for usability.
			<p class="mt-8 text-center text-sm">
				<a
					href="/auth/bitauth/logout"
					class="text-anakiwa underline-offset-2 hover:underline"
				>
					"Log out"
				</a>
			</p>
		</main>
	}
}
