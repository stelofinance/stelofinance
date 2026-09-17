//! SpacetimeAuth OIDC for minting **app** identities. Never writes BitAuth cookies.

use crate::auth::bitauth::AuthStart;
use openidconnect::{
	AuthorizationCode, ClientId, CsrfToken, EndpointMaybeSet, EndpointNotSet, EndpointSet,
	IssuerUrl, Nonce, OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope,
	TokenResponse,
	core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata, CoreTokenResponse},
	reqwest,
};
use spacetimedb_sdk::Identity;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type OidcClient = CoreClient<
	EndpointSet,
	EndpointNotSet,
	EndpointNotSet,
	EndpointNotSet,
	EndpointMaybeSet,
	EndpointMaybeSet,
>;

const MINT_TTL: Duration = Duration::from_secs(5 * 60);
const DEFAULT_ISSUER: &str = "https://auth.spacetimedb.com/oidc";

/// Optional SpacetimeAuth client + one-time mint flash (process-local).
#[derive(Clone)]
pub struct SpacetimeAuthState {
	pub client: Option<SpacetimeAuth>,
	pub mints: MintStore,
}

#[derive(Clone)]
pub struct SpacetimeAuth {
	inner: Arc<SpacetimeAuthInner>,
}

struct SpacetimeAuthInner {
	client: OidcClient,
	http: reqwest::Client,
}

pub struct AppTokens {
	pub id_token: String,
	pub refresh_token: Option<String>,
	pub sub: String,
}

#[derive(Clone)]
pub struct MintFlash {
	pub id_token: String,
	pub refresh_token: Option<String>,
	#[allow(dead_code)]
	pub sub: String,
	pub name: String,
	pub identity_hex: Option<String>,
	pub user: Identity,
}

#[derive(Clone, Default)]
pub struct MintStore {
	inner: Arc<Mutex<HashMap<String, (Instant, MintFlash)>>>,
}

impl MintStore {
	pub fn put(&self, flash: MintFlash) -> String {
		self.evict();
		let mut id = [0u8; 16];
		let _ = getrandom::fill(&mut id);
		let key = id.iter().map(|b| format!("{b:02x}")).collect::<String>();
		self.inner
			.lock()
			.unwrap()
			.insert(key.clone(), (Instant::now() + MINT_TTL, flash));
		key
	}

	pub fn take(&self, key: &str) -> Option<MintFlash> {
		self.evict();
		self.inner
			.lock()
			.unwrap()
			.remove(key)
			.filter(|(until, _)| *until > Instant::now())
			.map(|(_, f)| f)
	}

	fn evict(&self) {
		let now = Instant::now();
		self.inner
			.lock()
			.unwrap()
			.retain(|_, (until, _)| *until > now);
	}
}

impl SpacetimeAuthState {
	pub async fn from_env() -> Self {
		match SpacetimeAuth::try_from_env().await {
			Ok(Some(client)) => {
				eprintln!("spacetimeauth: OIDC client ready");
				Self {
					client: Some(client),
					mints: MintStore::default(),
				}
			}
			Ok(None) => {
				eprintln!("spacetimeauth: not configured (New app hidden)");
				Self {
					client: None,
					mints: MintStore::default(),
				}
			}
			Err(e) => {
				eprintln!("spacetimeauth: config error, New app hidden: {e}");
				Self {
					client: None,
					mints: MintStore::default(),
				}
			}
		}
	}

	pub fn configured(&self) -> bool {
		self.client.is_some()
	}
}

impl SpacetimeAuth {
	pub async fn try_from_env() -> Result<Option<Self>, String> {
		let client_id = optional_env("SPACETIMEAUTH_CLIENT_ID");
		let client_secret = optional_env("SPACETIMEAUTH_CLIENT_SECRET");
		let redirect = optional_env("SPACETIMEAUTH_REDIRECT_URL");
		match (client_id, client_secret, redirect) {
			(None, None, None) => Ok(None),
			(Some(client_id), Some(_client_secret), Some(redirect)) => {
				// Secret is required in env so the dashboard copy is complete, but
				// SpacetimeAuth only uses it for `client_credentials`. Authorization
				// code + PKCE is a public client; sending Basic auth with the secret
				// returns `invalid_client`.
				Ok(Some(Self::connect(client_id, redirect).await?))
			}
			_ => Err(
				"SPACETIMEAUTH_CLIENT_ID, SPACETIMEAUTH_CLIENT_SECRET, and SPACETIMEAUTH_REDIRECT_URL must be set together"
					.to_owned(),
			),
		}
	}

	async fn connect(client_id: String, redirect: String) -> Result<Self, String> {
		// Discovery `issuer` is slash-free (`…/oidc`). A trailing slash fails
		// openidconnect's exact-match check (BitAuth's issuer *does* use `/`).
		let issuer = normalize_issuer(
			&std::env::var("SPACETIMEAUTH_ISSUER")
				.ok()
				.map(|s| s.trim().to_owned())
				.filter(|s| !s.is_empty())
				.unwrap_or_else(|| DEFAULT_ISSUER.to_owned()),
		);

		let http = reqwest::ClientBuilder::new()
			.redirect(reqwest::redirect::Policy::none())
			.build()
			.map_err(|e| e.to_string())?;

		let issuer_url = IssuerUrl::new(issuer).map_err(|e| e.to_string())?;
		let provider_metadata = CoreProviderMetadata::discover_async(issuer_url, &http)
			.await
			.map_err(|e| format!("OIDC discovery: {e}"))?;

		let client =
			CoreClient::from_provider_metadata(provider_metadata, ClientId::new(client_id), None)
				.set_redirect_uri(RedirectUrl::new(redirect).map_err(|e| e.to_string())?);

		Ok(Self {
			inner: Arc::new(SpacetimeAuthInner { client, http }),
		})
	}

	pub fn auth_start(&self) -> AuthStart {
		let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
		let (authorize_url, csrf_token, nonce) = self
			.inner
			.client
			.authorize_url(
				CoreAuthenticationFlow::AuthorizationCode,
				CsrfToken::new_random,
				Nonce::new_random,
			)
			.add_scope(Scope::new("openid".to_owned()))
			.add_scope(Scope::new("offline_access".to_owned()))
			.set_pkce_challenge(pkce_challenge)
			.url();

		AuthStart {
			authorize_url,
			state: csrf_token.secret().clone(),
			nonce: nonce.secret().clone(),
			pkce_verifier: pkce_verifier.secret().clone(),
		}
	}

	pub async fn exchange_code(
		&self,
		code: &str,
		pkce_verifier: &str,
		nonce: &str,
	) -> Result<AppTokens, String> {
		let token_response: CoreTokenResponse = self
			.inner
			.client
			.exchange_code(AuthorizationCode::new(code.to_owned()))
			.map_err(|e| e.to_string())?
			.set_pkce_verifier(PkceCodeVerifier::new(pkce_verifier.to_owned()))
			.request_async(&self.inner.http)
			.await
			.map_err(|e| e.to_string())?;

		let id_token = token_response
			.id_token()
			.ok_or_else(|| "provider did not return an id_token".to_owned())?
			.clone();

		let claims = id_token
			.claims(
				&self.inner.client.id_token_verifier(),
				&Nonce::new(nonce.to_owned()),
			)
			.map_err(|e| format!("id_token verify: {e}"))?;

		Ok(AppTokens {
			id_token: id_token.to_string(),
			refresh_token: token_response.refresh_token().map(|t| t.secret().clone()),
			sub: claims.subject().to_string(),
		})
	}
}

fn optional_env(key: &str) -> Option<String> {
	std::env::var(key)
		.ok()
		.map(|s| s.trim().to_owned())
		.filter(|s| !s.is_empty())
}

fn normalize_issuer(raw: &str) -> String {
	raw.trim().trim_end_matches('/').to_owned()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn spacetimeauth_issuer_has_no_trailing_slash() {
		assert_eq!(
			normalize_issuer("https://auth.spacetimedb.com/oidc/"),
			"https://auth.spacetimedb.com/oidc"
		);
		assert_eq!(
			normalize_issuer("https://auth.spacetimedb.com/oidc"),
			"https://auth.spacetimedb.com/oidc"
		);
		assert_eq!(normalize_issuer(DEFAULT_ISSUER), DEFAULT_ISSUER);
	}
}
