//! Marketing illustrations ported from `web/templates/illustrations/`.

use topcoat::{
	Result,
	view::{View, component, view},
};

const NINTRON_PROFILE: &str = include_str!("svg/nintron-profile.svg");
const NINTRON_HEX_BODY: &str = include_str!("svg/nintron-hex.svg");

/// Nintron contribute-section figure (profile + spinning hex).
#[component]
pub async fn nintron(#[into] class: String) -> Result {
	view! {
		<div class=(format!("relative {class}"))>
			(View::unescaped_unchecked(NINTRON_PROFILE))
			<svg
				class="absolute animate-spin-slow"
				aria-label="Interconnected spinning hexagon"
				style="right: 0; top: 27%;"
				width="40"
				height="46"
				viewBox="0 0 40 46"
				fill="none"
				xmlns="http://www.w3.org/2000/svg"
			>
				(View::unescaped_unchecked(NINTRON_HEX_BODY))
			</svg>
		</div>
	}
}
