use openidconnect::{
	AuthorizationCode, ClaimsVerificationError, ClientId, ClientSecret, CsrfToken,
	EndpointMaybeSet, EndpointNotSet, EndpointSet, IssuerUrl, Nonce, OAuth2TokenResponse,
	PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken, Scope, TokenResponse,
	core::{
		CoreAuthenticationFlow, CoreClient, CoreIdToken, CoreProviderMetadata, CoreTokenResponse,
	},
	reqwest,
	url::Url,
};
use std::str::FromStr;
use std::sync::Arc;

type OidcClient = CoreClient<
	EndpointSet,
	EndpointNotSet,
	EndpointNotSet,
	EndpointNotSet,
	EndpointMaybeSet,
	EndpointMaybeSet,
>;

/// Shared OIDC client for user login (app context). The provider is whatever
/// `OIDC_ISSUER` points at; it must match the module's `USER_OIDC_ISSUER`.
#[derive(Clone)]
pub struct OidcAuth {
	inner: Arc<OidcAuthInner>,
}

struct OidcAuthInner {
	client: OidcClient,
	http: reqwest::Client,
	provider_name: Option<String>,
}

pub struct AuthStart {
	pub authorize_url: Url,
	pub state: String,
	pub nonce: String,
	pub pkce_verifier: String,
}

/// Why a stored ID token was not accepted by [`OidcAuth::verify_id_token`].
#[derive(Debug)]
pub enum TokenRejected {
	/// Signature and claims were fine but `exp` has passed. Routine; refresh.
	Expired,
	/// Malformed, bad signature, wrong issuer / audience, etc.
	Invalid(String),
}

pub struct AuthTokens {
	/// Raw ID token JWT (cookie + later STDB connect).
	pub id_token: String,
	pub refresh_token: Option<String>,
}

impl OidcAuth {
	/// Configure from `OIDC_ISSUER`, `OIDC_CLIENT_ID`, `OIDC_CLIENT_SECRET`,
	/// `OIDC_REDIRECT_URL` (required) and `OIDC_PROVIDER_NAME` (optional, shown
	/// to users), then run discovery.
	pub async fn from_env() -> Result<Self, String> {
		let client_id = env_required("OIDC_CLIENT_ID")?;
		let client_secret = env_required("OIDC_CLIENT_SECRET")?;
		let redirect = env_required("OIDC_REDIRECT_URL")?;
		let provider_name = env_required("OIDC_PROVIDER_NAME").ok();

		let mut issuer = env_required("OIDC_ISSUER")?;
		if !issuer.ends_with('/') {
			issuer.push('/');
		}

		let http = reqwest::ClientBuilder::new()
			.redirect(reqwest::redirect::Policy::none())
			.build()
			.map_err(|e| e.to_string())?;

		let issuer_url = IssuerUrl::new(issuer).map_err(|e| e.to_string())?;

		let provider_metadata = CoreProviderMetadata::discover_async(issuer_url, &http)
			.await
			.map_err(|e| format!("OIDC discovery: {e}"))?;

		let client = CoreClient::from_provider_metadata(
			provider_metadata,
			ClientId::new(client_id),
			Some(ClientSecret::new(client_secret)),
		)
		.set_redirect_uri(RedirectUrl::new(redirect).map_err(|e| e.to_string())?);

		Ok(Self {
			inner: Arc::new(OidcAuthInner {
				client,
				http,
				provider_name,
			}),
		})
	}

	/// Display name of the login provider (`OIDC_PROVIDER_NAME`), if set.
	pub fn provider_name(&self) -> Option<&str> {
		self.inner.provider_name.as_deref()
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
			.add_scope(Scope::new("profile".to_owned()))
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
	) -> Result<AuthTokens, String> {
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

		// Verify signature / iss / aud / exp / nonce.
		id_token
			.claims(
				&self.inner.client.id_token_verifier(),
				&Nonce::new(nonce.to_owned()),
			)
			.map_err(|e| format!("id_token verify: {e}"))?;

		Ok(AuthTokens {
			id_token: id_token.to_string(),
			refresh_token: token_response.refresh_token().map(|t| t.secret().clone()),
		})
	}

	/// Verify a stored ID token (signature against the provider's JWKS, iss,
	/// aud, exp) and return its `exp` as Unix seconds. No nonce check: cookie
	/// tokens are not tied to one authorize request.
	///
	/// The JWKS is the one fetched at startup discovery.
	pub fn verify_id_token(&self, token: &str) -> Result<u64, TokenRejected> {
		let id_token =
			CoreIdToken::from_str(token).map_err(|e| TokenRejected::Invalid(e.to_string()))?;
		let claims = id_token
			.claims(
				&self.inner.client.id_token_verifier(),
				|_: Option<&Nonce>| Ok(()),
			)
			.map_err(|e| match e {
				ClaimsVerificationError::Expired(_) => TokenRejected::Expired,
				e => TokenRejected::Invalid(e.to_string()),
			})?;
		Ok(u64::try_from(claims.expiration().timestamp()).unwrap_or(0))
	}

	/// Exchange a refresh token for a new ID token (and possibly a rotated refresh token).
	///
	/// Verifies the new ID token signature / iss / aud / exp. Nonce is not checked
	/// (refresh responses are not bound to the original authorize nonce).
	pub async fn refresh(&self, refresh_token: &str) -> Result<AuthTokens, String> {
		let token_response: CoreTokenResponse = self
			.inner
			.client
			.exchange_refresh_token(&RefreshToken::new(refresh_token.to_owned()))
			.map_err(|e| format!("refresh config: {e}"))?
			.add_scope(Scope::new("openid".to_owned()))
			.add_scope(Scope::new("profile".to_owned()))
			.request_async(&self.inner.http)
			.await
			.map_err(|e| format!("refresh request: {e}"))?;

		let id_token = token_response
			.id_token()
			.ok_or_else(|| "provider did not return an id_token on refresh".to_owned())?
			.clone();

		id_token
			.claims(
				&self.inner.client.id_token_verifier(),
				|_: Option<&Nonce>| Ok(()),
			)
			.map_err(|e| format!("id_token verify (refresh): {e}"))?;

		Ok(AuthTokens {
			id_token: id_token.to_string(),
			refresh_token: token_response.refresh_token().map(|t| t.secret().clone()),
		})
	}
}

fn env_required(key: &'static str) -> Result<String, String> {
	std::env::var(key)
		.map(|s| s.trim().to_owned())
		.ok()
		.filter(|s| !s.is_empty())
		.ok_or_else(|| format!("missing env {key}"))
}
