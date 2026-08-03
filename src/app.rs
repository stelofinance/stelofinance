use topcoat::{
    Result,
    asset::{AssetBundle, RouterBuilderAssetExt, asset},
    font::{Font, font},
    router::{Router, RouterBuilderDiscoverExt, layout, module_router, page},
    tailwind,
    view::view,
};

const SOURCE_CODE_PRO: Font = font! {
    "Source Code Pro",
    @font-face {
        src: url(asset!("https://cdn.jsdelivr.net/fontsource/fonts/source-code-pro:vf@5.3.0/latin-wght-normal.woff2")) format("woff2") tech("variations");
        font-weight: 200 900;
        font-style: normal;
        font-display: swap;
    }
    @font-face {
        src: url(asset!("https://cdn.jsdelivr.net/fontsource/fonts/source-code-pro:vf@5.3.0/latin-wght-italic.woff2")) format("woff2") tech("variations");
        font-weight: 200 900;
        font-style: italic;
        font-display: swap;
    }
};

/// Build the HTTP router for the lite edge.
pub fn router() -> Router {
    module_router!()
        // Explicit-path handlers, fonts, etc. collected at link time.
        .discover()
        // Serves content-hashed assets under `/_topcoat/assets/...`.
        // Produce the bundle with `topcoat asset bundle` or `topcoat dev`.
        .assets(
            AssetBundle::load()
                .expect("asset bundle missing — run `topcoat asset bundle` or `topcoat dev`"),
        )
        .build()
}

/// HTML shell: head assets + public body chrome for every page under this module tree.
#[layout]
async fn root_layout(slot: Result) -> Result {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="UTF-8">
                <meta name="viewport" content="width=device-width, initial-scale=1.0">
                <title>"Stelo Finance"</title>
                <meta
                    name="description"
                    content="A finance platform for the game BitCraft. Free for any to use or build apps on through our API."
                >
                <link rel="icon" href=(asset!("assets/favicon.png"))>
                topcoat::font::link(font: SOURCE_CODE_PRO)
                <link rel="stylesheet" href=(tailwind::stylesheet!())>
                // Hot-reload client when running under `topcoat dev`.
                topcoat::dev::script()
            </head>
            <body class="min-h-dvh bg-neutral-900 font-source-code-pro text-white">
                (slot?)
            </body>
        </html>
    }
}

/// Placeholder marketing homepage (full content ported later).
#[page]
async fn home() -> Result {
    view! {
        <main class="flex min-h-dvh flex-col items-center justify-center gap-4 px-4">
            <p class="text-sm uppercase tracking-widest text-anakiwa">
                "Stelo Finance"
            </p>
            <h1 class="text-center text-3xl font-medium text-melrose lg:text-4xl">
                "Edge skeleton"
            </h1>
            <p class="max-w-lg text-center text-neutral-300">
                "Topcoat layout, Stelo theme, Source Code Pro, and favicon are wired. "
                "BitAuth and SpacetimeDB come next."
            </p>
            <p class="text-sm text-neutral-500">
                "lite webserver · Rust / Topcoat"
            </p>
        </main>
    }
}
