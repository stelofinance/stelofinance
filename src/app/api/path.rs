//! `/{*path}` under `/api` — thin reverse-proxy onto module HTTP.

use crate::stdb::StdbState;
use topcoat::{
	Result,
	context::{Cx, app_context},
	router::{
		HeaderMap, HeaderValue, RawPathParamValue, StatusCode, header, raw_path_params,
		request::{Bytes, headers, method, uri},
		route, segment,
	},
};

segment!(kind = CatchAll, rename = "path");

const UPSTREAM_UNAVAILABLE: &str = r#"{"error":"upstream unavailable"}"#;
const BAD_PATH: &str = r#"{"error":"invalid path"}"#;

/// Forward method, query, body, and a small header allowlist to module HTTP.
#[route(*)]
async fn proxy(cx: &Cx, body: Bytes) -> Result<(StatusCode, HeaderMap, Bytes)> {
	let Some(path) = catch_all_path(cx) else {
		return Ok(json_status(StatusCode::BAD_REQUEST, BAD_PATH));
	};

	let stdb = app_context::<StdbState>(cx);
	let url = stdb.config.module_route_url(path, uri(cx).query());

	let mut req = stdb.http.request(method(cx).clone(), url);
	req = copy_allowlisted_headers(req, headers(cx));
	if !body.is_empty() {
		req = req.body(body);
	}

	let upstream = match req.send().await {
		Ok(r) => r,
		Err(_) => return Ok(json_status(StatusCode::BAD_GATEWAY, UPSTREAM_UNAVAILABLE)),
	};

	let status =
		StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
	let mut out_headers = HeaderMap::new();
	if let Some(ct) = upstream.headers().get(header::CONTENT_TYPE) {
		if let Ok(v) = HeaderValue::from_bytes(ct.as_bytes()) {
			out_headers.insert(header::CONTENT_TYPE, v);
		}
	}
	let bytes = match upstream.bytes().await {
		Ok(b) => b,
		Err(_) => return Ok(json_status(StatusCode::BAD_GATEWAY, UPSTREAM_UNAVAILABLE)),
	};

	Ok((status, out_headers, bytes))
}

/// Encoded catch-all tail, after rejecting empty, `.`, and `..` segments.
///
/// `raw_path_params` yields a [`RawPathParamValue`]: the tail keeps the
/// client's percent-encoding (so it can be placed in the upstream URL), and
/// the segments are decoded for the safety check.
fn catch_all_path(cx: &Cx) -> Option<&str> {
	let (_, value) = raw_path_params(cx).find(|(name, _)| *name == "path")?;
	let RawPathParamValue::CatchAll { tail, segments } = value else {
		return None;
	};
	let safe = segments
		.clone()
		.all(|seg| !seg.is_empty() && seg != "." && seg != "..");
	safe.then_some(tail)
}

fn copy_allowlisted_headers(
	mut req: reqwest::RequestBuilder,
	incoming: &HeaderMap,
) -> reqwest::RequestBuilder {
	for name in [header::AUTHORIZATION, header::CONTENT_TYPE] {
		if let Some(value) = incoming.get(&name) {
			req = req.header(name, value);
		}
	}
	if let Some(value) = incoming.get("idempotency-key") {
		req = req.header("Idempotency-Key", value);
	}
	req
}

fn json_status(status: StatusCode, body: &'static str) -> (StatusCode, HeaderMap, Bytes) {
	let mut headers = HeaderMap::new();
	headers.insert(
		header::CONTENT_TYPE,
		HeaderValue::from_static("application/json"),
	);
	(status, headers, Bytes::from_static(body.as_bytes()))
}
