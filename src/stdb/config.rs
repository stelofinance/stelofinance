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

	/// Host endpoint that mints a fresh identity + locally-signed token: `$STDB/v1/identity`.
	pub fn identity_url(&self) -> String {
		format!("{}/v1/identity", self.host.trim_end_matches('/'))
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
	fn identity_url_strips_host_slash() {
		assert_eq!(cfg().identity_url(), "http://127.0.0.1:3000/v1/identity");
	}
}
