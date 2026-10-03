//! `GET /health` — edge process liveness. No STDB.

use topcoat::{Result, router::route};

/// Cheap Fly / deploy probe. Not the module ping (`GET /api/ping`).
#[route(GET)]
async fn health() -> Result<&'static str> {
	Ok("ok")
}
