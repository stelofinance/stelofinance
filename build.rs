fn main() {
	topcoat::tailwind::BuildConfig::new()
		.input("src/styles/app.css")
		.executable("tailwindcss")
		.render()
		.unwrap();

	// Iconify sets used via `iconify_icon!("set:name")` at compile time.
	topcoat::icon::iconify::BuildConfig::new()
		.icon_set("lucide")
		.stage()
		.unwrap();
}
