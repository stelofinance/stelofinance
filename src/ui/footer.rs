//! Public marketing footer.

use topcoat::{
	Result,
	view::{component, view},
};

use super::icons::logo_full;

struct FooterLink {
	href: &'static str,
	text: &'static str,
}

const FOOTER_LINKS: &[FooterLink] = &[
	FooterLink {
		href: "https://discord.gg/t6gM7v7V7T",
		text: "Discord",
	},
	FooterLink {
		href: "https://github.com/stelofinance/stelofinance/tree/main/docs",
		text: "Docs",
	},
	FooterLink {
		href: "https://github.com/stelofinance",
		text: "GitHub",
	},
];

#[component]
pub async fn public_footer() -> Result {
	view! {
		<footer class="relative flex items-center gap-4 px-3 pt-4 pb-8 text-white lg:gap-10 lg:px-16 lg:pt-6 lg:pb-10 2xl:gap-12">
			logo_full(class: "mr-auto h-auto w-24 lg:w-32 2xl:w-36")
			for link in FOOTER_LINKS {
				<a
					class="text-sm lg:text-base 2xl:text-lg"
					href=(link.href)
					rel="noopener noreferrer"
					target="_blank"
				>
					(link.text)
				</a>
			}
			<p class="absolute bottom-2 left-1/2 -translate-x-1/2 text-xs text-nowrap text-neutral-300 lg:bottom-3">
				"Not affiliated with Clockwork Labs"
			</p>
		</footer>
	}
}
