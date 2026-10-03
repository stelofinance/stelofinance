/// Parameters passed to [`super::Connector::connect`].
#[derive(Debug, Clone, Copy)]
pub struct ConnectParams<'a> {
	pub uri: &'a str,
	pub database: &'a str,
	pub token: &'a str,
}
