//! Public marketing navigation.

use topcoat::{
	Result,
	view::{View, component, view},
};

use super::icons::logo_full;

const HEADER_OFFSET_CSS: &str = r#"
* {
	--header-offset: 61.3px;
}
@media (min-width: 1024px) {
	* {
		--header-offset: 77.3px;
	}
}
@media (min-width: 1536px) {
	* {
		--header-offset: 83.76px;
	}
}
"#;

/// Sticky top nav with logo and header-offset CSS vars (for `h-screen-available`).
#[component]
pub async fn public_nav() -> Result {
	view! {
		<nav
			class="sticky top-0 z-30 flex items-center gap-3 bg-neutral-900 px-3 py-2 text-white lg:px-10 lg:py-4 2xl:px-20"
			aria-label="Main"
		>
			<a href="/" aria-label="Go to homepage">
				logo_full(class: "mr-auto h-auto w-28 2xl:w-32")
			</a>
			<style>
				(View::unescaped_unchecked(HEADER_OFFSET_CSS))
			</style>
		</nav>
	}
}
