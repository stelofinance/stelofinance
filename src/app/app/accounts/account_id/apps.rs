//! `GET /app/accounts/{account_id}/apps` — app name search for add-permission.

use super::account_id;
use crate::ui::text::escape_html;
use crate::auth::require_user;
use crate::module_bindings::AppSearchHit;
use crate::stdb::{StdbError, acquire_user_db, search_apps};
use serde::Deserialize;
use topcoat::{
	Result,
	context::Cx,
	datastar::{PatchElements, Signals},
	router::{error::internal_server_error, route},
};

#[derive(Debug, Deserialize)]
struct SearchSignals {
	#[serde(default, rename = "appSearch")]
	app_search: String,
}

#[route(GET)]
async fn app_search_route(cx: &Cx, Signals(form): Signals<SearchSignals>) -> Result<PatchElements> {
	let _user = require_user(cx).await?;
	let _account_id = *account_id(cx)?;
	let term = form.app_search.trim();
	if term.is_empty() {
		return Ok(PatchElements::new(results_html(&[], false)));
	}

	let conn = acquire_user_db(cx).await?;
	let rows = search_apps(conn.get(), term)
		.await
		.map_err(|e| internal_server_error(StdbError(e)))?;
	Ok(PatchElements::new(results_html(&rows, true)))
}

fn results_html(rows: &[AppSearchHit], searched: bool) -> String {
	if !searched {
		return String::from(r#"<div id="app-search-results"></div>"#);
	}
	let mut out = String::from(
		r#"<div id="app-search-results" class="mt-2 flex flex-col overflow-hidden rounded-md border border-neutral-800 bg-neutral-900">"#,
	);
	if rows.is_empty() {
		out.push_str(
			r#"<p class="px-3 py-2 text-sm text-neutral-400">No apps match that name.</p>"#,
		);
	} else {
		for row in rows {
			let name = escape_html(&row.name);
			let hex = row.id.to_hex();
			let owner = row
				.owner_username
				.as_deref()
				.map(str::trim)
				.filter(|s| !s.is_empty());
			let owner_html = owner
				.map(|u| {
					format!(
						r#"<span class="text-neutral-500"> · {}</span>"#,
						escape_html(u)
					)
				})
				.unwrap_or_default();
			out.push_str(&format!(
				r#"<button type="button" class="cursor-pointer px-3 py-2 text-left text-sm text-neutral-200 hover:bg-neutral-800" data-name="{name}" data-on:click="$memberId = '{hex}'; $memberName = el.dataset.name; $appSearch = ''">{name}{owner_html}</button>"#
			));
		}
	}
	out.push_str("</div>");
	out
}

