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
}
