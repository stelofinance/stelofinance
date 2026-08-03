use topcoat::{Result, router::page, view::view};

/// Login page: BitAuth only (no BitJita).
#[page]
async fn login() -> Result {
	view! {
		<main class="flex h-screen-available flex-col items-center justify-center px-4 text-white">
			<h1 class="text-4xl font-medium">"Login"</h1>
			<a
				href="/auth/bitauth/login"
				class="mt-16 flex items-center gap-2 rounded-md px-3 py-2 text-white hover:shadow-md lg:px-6 lg:py-3 lg:text-lg"
				style="background-color: #15567E;"
			>
				"Login with BitAuth"
			</a>
			<p class="mb-40 mt-4 max-w-2xl px-4 text-center text-sm text-neutral-400">
				"Stelo is currently in a beta state, and as such you should expect an unfinished experience, "
				"and occasional bugs or service interruption."
			</p>
		</main>
	}
}
