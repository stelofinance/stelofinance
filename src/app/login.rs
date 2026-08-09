use crate::auth::cookies::is_valid_redirect;
use crate::ui::{public_footer, public_nav};
use topcoat::{
	Result,
	context::Cx,
	router::{page, query_params},
	view::view,
};

#[query_params]
struct LoginPageQuery {
	redirect: Option<String>,
}

/// Login page: BitAuth only (no BitJita).
///
/// Forwards a safe `?redirect=` to the OIDC start so `/app` gates can return
/// the user to the page they wanted after sign-in.
#[page]
async fn login(cx: &Cx) -> Result {
	let bitauth_href = match query_params::<LoginPageQuery>(cx) {
		Ok(q) => match q.redirect.as_deref() {
			Some(r) if is_valid_redirect(r) => format!("/auth/bitauth/login?redirect={r}"),
			_ => "/auth/bitauth/login".to_owned(),
		},
		Err(_) => "/auth/bitauth/login".to_owned(),
	};

	view! {
		public_nav()
		<main class="flex h-screen-available flex-col items-center justify-center px-4 text-white">
			<h1 class="text-4xl font-medium">"Login"</h1>
			<a
				href=(bitauth_href)
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
		public_footer()
	}
}
