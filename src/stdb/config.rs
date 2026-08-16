#[derive(Clone, Debug)]
pub struct StdbConfig {
	pub host: String,
	pub database: String,
}

impl StdbConfig {
	pub fn from_env() -> Self {
		let host = std::env::var("STDB_HOST")
			.ok()
			.map(|s| s.trim().to_owned())
			.filter(|s| !s.is_empty())
			.unwrap_or_else(|| "http://127.0.0.1:3000".to_owned());

		let database = std::env::var("STDB_DATABASE")
			.ok()
			.map(|s| s.trim().to_owned())
			.filter(|s| !s.is_empty())
			.unwrap_or_else(|| "stelofinance".to_owned());

		Self { host, database }
	}

	/// Upstream URL for a module HTTP route: `$STDB/v1/database/$DB/route/{path}`.
	///
	/// `path` is the remainder after the public `/api/` prefix (no leading slash).
	pub fn module_route_url(&self, path: &str, query: Option<&str>) -> String {
		let host = self.host.trim_end_matches('/');
		let path = path.trim_start_matches('/');
		let mut url = format!("{host}/v1/database/{}/route/{path}", self.database);
		if let Some(q) = query.filter(|q| !q.is_empty()) {
			url.push('?');
			url.push_str(q);
		}
		url
	}
}

#[cfg(test)]
mod tests {
	use super::StdbConfig;

	fn cfg() -> StdbConfig {
		StdbConfig {
			host: "http://127.0.0.1:3000/".to_owned(),
			database: "stelofinance".to_owned(),
		}
	}

	#[test]
	fn module_route_url_strips_host_slash_and_path_slash() {
		assert_eq!(
			cfg().module_route_url("/ping", None),
			"http://127.0.0.1:3000/v1/database/stelofinance/route/ping"
		);
	}

	#[test]
	fn module_route_url_keeps_nested_path_and_query() {
		assert_eq!(
			cfg().module_route_url("account/transfers", Some("limit=10&offset=0")),
			"http://127.0.0.1:3000/v1/database/stelofinance/route/account/transfers?limit=10&offset=0"
		);
	}

	#[test]
	fn module_route_url_drops_empty_query() {
		assert_eq!(
			cfg().module_route_url("account/ping", Some("")),
			"http://127.0.0.1:3000/v1/database/stelofinance/route/account/ping"
		);
	}
}
