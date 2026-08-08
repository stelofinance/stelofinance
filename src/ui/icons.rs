//! Brand and social icons ported from `web/templates/icons/`.
//!
//! SVG path bodies live in `svg/` so page code stays free of path markup.

use topcoat::{
	Result,
	icon::{IconData, icon},
	view::{View, attributes, component, svg::ViewBox, view},
};

const LOGO_COLORED_BODY: &str = include_str!("svg/logo-colored.svg");
const LOGO_FULL_BODY: &str = include_str!("svg/logo-full.svg");

const RIGHT_ARROW: IconData = IconData::unescaped_unchecked(
	ViewBox::new(0.0, 0.0, 20.0, 20.0),
	include_str!("svg/right-arrow.svg"),
);

const GITHUB: IconData = IconData::unescaped_unchecked(
	ViewBox::new(0.0, 0.0, 27.0, 27.0),
	include_str!("svg/github.svg"),
);

const DISCORD: IconData = IconData::unescaped_unchecked(
	ViewBox::new(0.0, 0.0, 24.0, 24.0),
	include_str!("svg/discord.svg"),
);

/// Colored hexagonal star (hero). Non-square; size via `class`.
#[component]
pub async fn logo_colored(#[into] class: String) -> Result {
	view! {
		<svg
			class=(class)
			aria-label="Stelo icon, a hexagonal star"
			width="1000"
			height="1156"
			viewBox="0 0 1000 1156"
			fill="none"
			xmlns="http://www.w3.org/2000/svg"
		>
			(View::unescaped_unchecked(LOGO_COLORED_BODY))
		</svg>
	}
}

/// Wordmark + mark (nav/footer). Non-square; size via `class`.
#[component]
pub async fn logo_full(#[into] class: String) -> Result {
	view! {
		<svg
			class=(class)
			aria-label="Stelo icon, a hexagonal star with the word Stelo on the right"
			width="2858"
			height="1156"
			viewBox="0 0 2858 1156"
			fill="none"
			xmlns="http://www.w3.org/2000/svg"
		>
			(View::unescaped_unchecked(LOGO_FULL_BODY))
		</svg>
	}
}

#[component]
pub async fn right_arrow(#[into] class: String) -> Result {
	view! {
		icon(
			data: RIGHT_ARROW,
			label: "right arrow",
			attrs: attributes! { class=(class) },
		)
	}
}

#[component]
pub async fn github(#[into] class: String) -> Result {
	view! {
		icon(
			data: GITHUB,
			label: "GitHub Icon",
			attrs: attributes! { class=(class) },
		)
	}
}

#[component]
pub async fn discord(#[into] class: String) -> Result {
	view! {
		icon(
			data: DISCORD,
			label: "Discord Icon",
			attrs: attributes! { class=(class) },
		)
	}
}
