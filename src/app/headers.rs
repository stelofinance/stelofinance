//! Security headers on every response, including error pages.

use topcoat::{
	context::Cx,
	router::{
		Body, HeaderMap, HeaderName, HeaderValue, Layer, LayerFuture, Next, Path, header,
		response::response_headers,
	},
};

use crate::auth::cookies::secure_cookies;

/// No script or style restrictions yet. The Topcoat runtime compiles
/// expressions with `new Function` (needs `'unsafe-eval'`) and streams inline
/// `<script>`s for `live!` swaps without nonces (needs `'unsafe-inline'`), and
/// the nav uses inline `onmouseenter` handlers.
const CSP: &str = "frame-ancestors 'none'; base-uri 'self'; object-src 'none'";

/// One year. Only sent when cookies are `Secure` (not `ENV=dev`), since local
/// dev runs on plain HTTP.
const HSTS: &str = "max-age=31536000";

#[derive(Debug, Clone, Copy, Default)]
pub struct SecurityHeadersLayer;

impl Layer for SecurityHeadersLayer {
	fn path(&self) -> Option<&Path> {
		None
	}

	fn handle<'a>(&'a self, cx: &'a Cx, body: Body, next: Next<'a>) -> LayerFuture<'a> {
		Box::pin(async move {
			match next.run(cx, body).await {
				Ok(mut response) => {
					set_headers(response.headers_mut());
					Ok(response)
				}
				// Error responses are built later; queue the headers for them.
				Err(error) => {
					let mut headers = HeaderMap::new();
					set_headers(&mut headers);
					response_headers(cx).extend(headers);
					Err(error)
				}
			}
		})
	}
}

fn set_headers(headers: &mut HeaderMap) {
	let mut set = |name: HeaderName, value: &'static str| {
		headers
			.entry(name)
			.or_insert(HeaderValue::from_static(value));
	};
	set(header::X_CONTENT_TYPE_OPTIONS, "nosniff");
	set(header::X_FRAME_OPTIONS, "DENY");
	set(header::REFERRER_POLICY, "strict-origin-when-cross-origin");
	set(header::CONTENT_SECURITY_POLICY, CSP);
	if secure_cookies() {
		set(header::STRICT_TRANSPORT_SECURITY, HSTS);
	}
}
