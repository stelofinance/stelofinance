//! Shared Topcoat runtime widgets: behaviour outside the expression
//! vocabulary (clipboard, timers) lives here, so pages don't hand-write `raw!`.
//!
//! `raw!` notes (see AGENTS.md):
//! - Its JavaScript must be ONE expression. As a closure's trailing
//!   expression it compiles to `return <js>;`, so a second `;`-statement is
//!   dead code. Join steps with the comma operator inside parentheses.
//! - Interpolated values are runtime surrogates: `.toString()` a string, and
//!   set a signal with another surrogate bound in the expression.

use topcoat::{
	Result,
	context::Cx,
	runtime::{Event, Expr, Signal, signal},
	view::{View, component, view},
};

/// A "Copy" button that writes `text` to the clipboard and reads "Copied"
/// for two seconds. `text` is evaluated at click time, so it may read
/// signals. With `origin`, the page origin is prepended (for site paths).
#[component]
pub async fn copy_button(
	cx: &Cx,
	text: Expr<String>,
	#[default] origin: bool,
	class: &'static str,
	#[default] aria_label: Option<String>,
) -> Result<impl View> {
	let copied = signal(cx, || false);
	Ok(view! {
    <button
        type="button"
        class=(class)
        aria-label=(aria_label)
        @click=$(|_e| {
            copied.set(true);
            let off = false;
            if origin {
                raw!(
                    "(navigator.clipboard.writeText(window.location.origin + ${text}.toString()), setTimeout(() => ${copied}.set(${off}), 2000))",
                    {
                        let _ = (text, off);
                    },
                )
            } else {
                raw!(
                    "(navigator.clipboard.writeText(${text}.toString()), setTimeout(() => ${copied}.set(${off}), 2000))",
                    {
                        let _ = (text, off);
                    },
                )
            }
        })
    >
        $(if copied.get() { "Copied" } else { "Copy" })
    </button>
})
}

/// A text input that publishes to `query` once typing pauses (300ms), so a
/// shard reading `query` re-renders once per pause instead of per keystroke.
/// Setting `query` elsewhere (e.g. clearing it) also updates the input.
/// `placeholder` is an expression, so it can follow a signal.
#[component]
pub async fn search_input(
	cx: &Cx,
	query: Signal<String>,
	class: &'static str,
	placeholder: Expr<String>,
) -> Result<impl View> {
	let draft = signal(cx, String::new);
	Ok(view! {
    <input
        type="text"
        class=(class)
        :placeholder=(placeholder)
        autocomplete="off"
        :value=$(query.get())
        @input=$(|e: Event| {
            let v = e.target.value;
            draft.set(v.to_owned());
            // Publish only if nothing was typed in the meantime.
            raw!(
                "setTimeout(() => ${draft}.get().toString() === ${v}.toString() && ${query}.set(${v}), 300)",
                {
                    let _ = (v, draft, query);
                },
            )
        })
    >
})
}
