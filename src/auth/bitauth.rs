use openidconnect::{
	AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet, EndpointNotSet,
	EndpointSet, IssuerUrl, Nonce, OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier,
	ProviderMetadataWithLogout, RedirectUrl, RefreshToken, Scope, TokenResponse,
	core::{CoreAuthenticationFlow, CoreClient, CoreTokenResponse},
	reqwest,
	url::Url,
};
use std::sync::Arc;

type OidcClient = CoreClient<
	EndpointSet,
	EndpointNotSet,
	EndpointNotSet,
	EndpointNotSet,
	EndpointMaybeSet,
	EndpointMaybeSet,
>;

/// Shared BitAuth OIDC client (app context).
#[derive(Clone)]
pub struct BitAuth {
	inner: Arc<BitAuthInner>,
}

struct BitAuthInner {
	client: OidcClient,
	http: reqwest::Client,
	end_session_url: Option<Url>,
	post_logout_redirect: Option<String>,
}

pub struct AuthStart {
	pub authorize_url: Url,
	pub state: String,
	pub nonce: String,
	pub pkce_verifier: String,
}

pub struct AuthTokens {
	/// Raw ID token JWT (cookie + later STDB connect).
	pub id_token: String,
	pub refresh_token: Option<String>,
}

impl BitAuth {
	pub async fn from_env() -> Result<Self, String> {
		let client_id = env_required("BITAUTH_CLIENT_ID")?;
		let client_secret = env_required("BITAUTH_CLIENT_SECRET")?;
		let redirect = env_required("BITAUTH_REDIRECT_URL")?;

		let mut issuer = std::env::var("BITAUTH_ISSUER")
			.unwrap_or_else(|_| "https://auth.trinit.is/".to_owned());
		if !issuer.ends_with('/') {
			issuer.push('/');
		}

		let post_logout_redirect = std::env::var("BITAUTH_LOGOUT_REDIRECT_URL")
			.ok()
			.map(|s| s.trim().to_owned())
			.filter(|s| !s.is_empty());

		let http = reqwest::ClientBuilder::new()
			.redirect(reqwest::redirect::Policy::none())
			.build()
			.map_err(|e| e.to_string())?;

		let issuer_url = IssuerUrl::new(issuer).map_err(|e| e.to_string())?;

		// Discover with logout metadata so we get `end_session_endpoint` without a second fetch.
		let provider_metadata = ProviderMetadataWithLogout::discover_async(issuer_url, &http)
			.await
			.map_err(|e| format!("OIDC discovery: {e}"))?;

		let end_session_url = provider_metadata
			.additional_metadata()
			.end_session_endpoint
			.as_ref()
			.map(|ep| ep.url().clone());

		let client = CoreClient::from_provider_metadata(
			provider_metadata,
			ClientId::new(client_id),
			Some(ClientSecret::new(client_secret)),
		)
		.set_redirect_uri(RedirectUrl::new(redirect).map_err(|e| e.to_string())?);

		Ok(Self {
			inner: Arc::new(BitAuthInner {
				client,
				http,
				end_session_url,
				post_logout_redirect,
			}),
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

	pub fn end_session_url(&self, id_token_hint: Option<&str>) -> Option<Url> {
		let mut url = self.inner.end_session_url.clone()?;
		{
			let mut pairs = url.query_pairs_mut();
			if let Some(hint) = id_token_hint.filter(|s| !s.is_empty()) {
				pairs.append_pair("id_token_hint", hint);
			}
			if let Some(ref post) = self.inner.post_logout_redirect {
				pairs.append_pair("post_logout_redirect_uri", post);
			}
		}
		Some(url)
	}
}

fn env_required(key: &'static str) -> Result<String, String> {
	std::env::var(key)
		.map(|s| s.trim().to_owned())
		.ok()
		.filter(|s| !s.is_empty())
		.ok_or_else(|| format!("missing env {key}"))
}
