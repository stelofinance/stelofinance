fn main() {
    topcoat::tailwind::BuildConfig::new()
        .input("src/styles/app.css")
        .executable("tailwindcss")
        .render()
        .unwrap();
}
