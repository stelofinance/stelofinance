use crate::auth::oidc::OidcAuth;
use crate::auth::redirect::with_redirect;
use crate::ui::{public_footer, public_nav};
use topcoat::{
	Result,
	context::{Cx, context},
	metadata::Metadata,
	router::{page, query_params},
	view::{View, view},
};

#[query_params]
struct LoginPageQuery {
	redirect: Option<String>,
}

/// Login page: the configured OIDC provider only.
///
/// Forwards a safe `?redirect=` to the OIDC start so `/app` gates can return
/// the user to the page they wanted after sign-in.
#[page(with(Metadata::new().title("Log in · Stelo Finance")))]
async fn login(cx: &Cx) -> Result<impl View> {
	let target = query_params::<LoginPageQuery>(cx)
		.ok()
		.and_then(|q| q.redirect.clone())
		.unwrap_or_default();
	let oidc_href = with_redirect("/auth/oidc/login", &target);
	let label = match context::<OidcAuth>(cx).provider_name() {
		Some(name) => format!("Log in with {name}"),
		None => "Log in".to_owned(),
	};

	Ok(view! {
    public_nav()
    <main
        class="flex h-screen-available flex-col items-center justify-center px-4 text-white"
    >
        <h1 class="text-4xl font-medium">"Login"</h1>
        <a
            href=(oidc_href)
            class="mt-16 flex items-center gap-2 rounded-md px-3 py-2 text-white hover:shadow-md lg:px-6 lg:py-3 lg:text-lg"
            style="background-color: #15567E;"
        >
            (label)
        </a>
        <p class="mb-40 mt-4 max-w-2xl px-4 text-center text-sm text-neutral-300">
            "Stelo is currently in a beta state, and as such you should expect an unfinished experience, "
            "and occasional bugs or service interruption."
        </p>
    </main>
    public_footer()
})
}
