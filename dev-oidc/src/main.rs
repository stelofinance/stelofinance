//! Local OIDC provider for development: sign in to Stelo as any username,
//! without the production login provider.
//!
//! It implements the subset of OIDC the edge (`src/auth/oidc.rs`) and
//! SpacetimeDB use: discovery, JWKS, the authorization-code flow with PKCE,
//! and refresh tokens. ID tokens carry the claims the module reads
//! (`spacetimedb/src/lib.rs`): `sub` is a numeric player id, derived from the
//! username so the same name always maps to the same identity, and
//! `preferred_username` is the name typed on the login page.
//!
//! Development only: it accepts any client secret and signs in anyone.
//!
//! Configuration (environment):
//! - `DEV_OIDC_ISSUER`, default `http://127.0.0.1:9090/`. Must be `http://`
//!   and end with `/`; the server listens on its host and port. The module's
//!   `USER_OIDC_ISSUER` (`spacetime.dev.json`) and the edge's
//!   `OIDC_ISSUER` must be the same string.
//! - `DEV_OIDC_KEY`, default `tmp/dev-oidc/key.pem`. The RS256 signing key,
//!   created on first run and reused so issued tokens survive restarts.

use std::{
	collections::HashMap,
	convert::Infallible,
	error::Error,
	path::Path,
	sync::{Arc, Mutex},
	time::{SystemTime, UNIX_EPOCH},
};

use base64::{
	Engine,
	engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD as B64},
};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{
	Method, Request, Response, StatusCode, body::Incoming, header, server::conn::http1,
	service::service_fn,
};
use hyper_util::rt::TokioIo;
use rsa::{
	RsaPrivateKey,
	pkcs1v15::{Signature, SigningKey, VerifyingKey},
	pkcs8::{DecodePrivateKey, EncodePrivateKey, LineEnding},
	rand_core::{OsRng, RngCore},
	sha2::{Digest, Sha256},
	signature::{SignatureEncoding, Signer, Verifier},
	traits::PublicKeyParts,
};
use serde_json::{Value, json};
use tokio::net::TcpListener;

const DEFAULT_ISSUER: &str = "http://127.0.0.1:9090/";
const DEFAULT_KEY_PATH: &str = "tmp/dev-oidc/key.pem";
const TOKEN_TTL_SECS: u64 = 3600;
const CODE_TTL_SECS: u64 = 300;
const MAX_USERNAME_CHARS: usize = 32;
const MAX_RECENT: usize = 10;

type Res = Response<Full<Bytes>>;
type Params = HashMap<String, String>;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
	let mut issuer = std::env::var("DEV_OIDC_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.into());
	if !issuer.ends_with('/') {
		issuer.push('/');
	}
	let authority = issuer
		.strip_prefix("http://")
		.ok_or("DEV_OIDC_ISSUER must start with http://")?
		.trim_end_matches('/')
		.to_owned();
	let key_path = std::env::var("DEV_OIDC_KEY").unwrap_or_else(|_| DEFAULT_KEY_PATH.into());
	let key = load_or_create_key(Path::new(&key_path))?;

	let provider = Arc::new(Provider::new(issuer.clone(), key));
	let listener = TcpListener::bind(&authority)
		.await
		.map_err(|e| format!("bind {authority}: {e}"))?;
	eprintln!("dev-oidc: issuer {issuer}");
	serve(listener, provider).await
}

async fn serve(listener: TcpListener, provider: Arc<Provider>) -> Result<(), Box<dyn Error>> {
	loop {
		let (stream, _) = listener.accept().await?;
		let provider = provider.clone();
		tokio::spawn(async move {
			let service = service_fn(move |req| handle(provider.clone(), req));
			if let Err(e) = http1::Builder::new()
				.serve_connection(TokioIo::new(stream), service)
				.await
			{
				eprintln!("dev-oidc: connection error: {e}");
			}
		});
	}
}

fn load_or_create_key(path: &Path) -> Result<RsaPrivateKey, Box<dyn Error>> {
	if let Ok(pem) = std::fs::read_to_string(path) {
		return Ok(RsaPrivateKey::from_pkcs8_pem(&pem)?);
	}
	eprintln!("dev-oidc: generating signing key at {}", path.display());
	let key = RsaPrivateKey::new(&mut OsRng, 2048)?;
	if let Some(dir) = path.parent() {
		std::fs::create_dir_all(dir)?;
	}
	std::fs::write(path, key.to_pkcs8_pem(LineEnding::LF)?.as_bytes())?;
	Ok(key)
}

/// One pending authorization code.
struct Grant {
	client_id: String,
	redirect_uri: String,
	nonce: Option<String>,
	code_challenge: Option<String>,
	username: String,
	expires_at: u64,
}

struct Provider {
	issuer: String,
	kid: String,
	jwk: Value,
	signing: SigningKey<Sha256>,
	verifying: VerifyingKey<Sha256>,
	codes: Mutex<HashMap<String, Grant>>,
	/// Usernames signed in since start, newest first, for one-click login.
	recent: Mutex<Vec<String>>,
}

impl Provider {
	fn new(issuer: String, key: RsaPrivateKey) -> Self {
		let public = key.to_public_key();
		let n = public.n().to_bytes_be();
		let kid = B64.encode(&Sha256::digest(&n)[..8]);
		let jwk = json!({
			"kty": "RSA",
			"use": "sig",
			"alg": "RS256",
			"kid": kid,
			"n": B64.encode(&n),
			"e": B64.encode(public.e().to_bytes_be()),
		});
		Self {
			issuer,
			kid,
			jwk,
			verifying: VerifyingKey::new(public),
			signing: SigningKey::new(key),
			codes: Mutex::new(HashMap::new()),
			recent: Mutex::new(Vec::new()),
		}
	}

	fn discovery(&self) -> Value {
		let iss = &self.issuer;
		json!({
			"issuer": iss,
			"authorization_endpoint": format!("{iss}authorize"),
			"token_endpoint": format!("{iss}token"),
			"jwks_uri": format!("{iss}jwks"),
			"response_types_supported": ["code"],
			"subject_types_supported": ["public"],
			"id_token_signing_alg_values_supported": ["RS256"],
			"scopes_supported": ["openid", "profile", "offline_access"],
			"grant_types_supported": ["authorization_code", "refresh_token"],
			"token_endpoint_auth_methods_supported": ["client_secret_basic", "client_secret_post"],
			"code_challenge_methods_supported": ["S256"],
			"claims_supported": ["iss", "sub", "aud", "exp", "iat", "nonce", "preferred_username"],
		})
	}

	fn sign(&self, claims: &Value) -> String {
		let header = json!({ "alg": "RS256", "typ": "JWT", "kid": self.kid });
		let input = format!(
			"{}.{}",
			B64.encode(header.to_string()),
			B64.encode(claims.to_string())
		);
		let signature = self.signing.sign(input.as_bytes());
		format!("{input}.{}", B64.encode(signature.to_bytes()))
	}

	/// Claims of a token this provider signed, or `None`.
	fn verify(&self, token: &str) -> Option<Value> {
		let (input, signature) = token.rsplit_once('.')?;
		let signature = Signature::try_from(B64.decode(signature).ok()?.as_slice()).ok()?;
		self.verifying.verify(input.as_bytes(), &signature).ok()?;
		let (_, payload) = input.split_once('.')?;
		serde_json::from_slice(&B64.decode(payload).ok()?).ok()
	}

	/// Token endpoint response for `username`.
	fn tokens(&self, client_id: &str, username: &str, nonce: Option<&str>) -> Value {
		let now = unix_now();
		let mut claims = json!({
			"iss": self.issuer,
			"sub": player_id(username).to_string(),
			"aud": client_id,
			"iat": now,
			"exp": now + TOKEN_TTL_SECS,
			"preferred_username": username,
		});
		if let Some(nonce) = nonce {
			claims["nonce"] = json!(nonce);
		}
		let id_token = self.sign(&claims);
		// Stateless and non-expiring, so it survives restarts. No `sub`/`exp`,
		// so neither the edge nor SpacetimeDB would accept it as an ID token.
		let refresh_token = self.sign(&json!({
			"iss": self.issuer,
			"token_use": "refresh",
			"aud": client_id,
			"username": username,
			"iat": now,
		}));
		json!({
			"access_token": id_token,
			"token_type": "Bearer",
			"expires_in": TOKEN_TTL_SECS,
			"id_token": id_token,
			"refresh_token": refresh_token,
			"scope": "openid profile offline_access",
		})
	}

	fn login_page(&self, params: &Params) -> Res {
		if let Err(message) = check_authorize_params(params) {
			return text(StatusCode::BAD_REQUEST, message);
		}
		// Every authorize parameter rides along so POST /authorize sees them.
		let hidden: String = params
			.iter()
			.filter(|(k, _)| k.as_str() != "username")
			.map(|(k, v)| {
				format!(
					r#"<input type="hidden" name="{}" value="{}">"#,
					escape(k),
					escape(v)
				)
			})
			.collect();
		let recent: String = self
			.recent
			.lock()
			.unwrap()
			.iter()
			.map(|u| {
				format!(
					r#"<form method="post" action="authorize">{hidden}<button class="recent" name="username" value="{u}">{u}</button></form>"#,
					u = escape(u)
				)
			})
			.collect();
		let recent = if recent.is_empty() {
			String::new()
		} else {
			format!(r#"<p class="muted">Recent</p><div class="row">{recent}</div>"#)
		};
		html(format!(
			r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>dev-oidc sign in</title>
<style>
body {{ font: 15px system-ui, sans-serif; background: #0a0a0a; color: #e5e5e5; display: grid; place-items: center; min-height: 100vh; margin: 0; }}
main {{ width: min(360px, calc(100vw - 32px)); }}
h1 {{ font-size: 18px; margin: 0 0 4px; }}
.muted {{ color: #a3a3a3; font-size: 13px; }}
input, button {{ font: inherit; border-radius: 6px; padding: 8px 12px; border: 1px solid #404040; }}
input {{ width: 100%; box-sizing: border-box; background: #171717; color: inherit; margin: 12px 0 8px; }}
button {{ cursor: pointer; background: #2563eb; color: white; border-color: #2563eb; }}
.row {{ display: flex; flex-wrap: wrap; gap: 6px; }}
.recent {{ background: #262626; border-color: #404040; }}
</style></head>
<body><main>
<h1>Sign in (local dev)</h1>
<p class="muted">Any username works. The same name always gets the same identity.</p>
<form method="post" action="authorize">{hidden}
<input name="username" placeholder="Username" maxlength="{MAX_USERNAME_CHARS}" required autofocus autocomplete="off">
<button type="submit">Sign in</button>
</form>
{recent}
</main></body></html>"#
		))
	}

	fn authorize(&self, params: &Params) -> Res {
		if let Err(message) = check_authorize_params(params) {
			return text(StatusCode::BAD_REQUEST, message);
		}
		let username = params.get("username").map(|s| s.trim()).unwrap_or("");
		if username.is_empty()
			|| username.chars().count() > MAX_USERNAME_CHARS
			|| username.chars().any(char::is_control)
		{
			return text(
				StatusCode::BAD_REQUEST,
				"username must be 1 to 32 printable characters",
			);
		}

		let code = random_token();
		let redirect_uri = params["redirect_uri"].clone();
		self.codes.lock().unwrap().insert(
			code.clone(),
			Grant {
				client_id: params["client_id"].clone(),
				redirect_uri: redirect_uri.clone(),
				nonce: params.get("nonce").cloned(),
				code_challenge: params.get("code_challenge").cloned(),
				username: username.to_owned(),
				expires_at: unix_now() + CODE_TTL_SECS,
			},
		);
		{
			let mut recent = self.recent.lock().unwrap();
			recent.retain(|u| u != username);
			recent.insert(0, username.to_owned());
			recent.truncate(MAX_RECENT);
		}

		let mut reply = vec![("code", code.as_str())];
		if let Some(state) = params.get("state") {
			reply.push(("state", state));
		}
		let separator = if redirect_uri.contains('?') { '&' } else { '?' };
		let location = format!(
			"{redirect_uri}{separator}{}",
			serde_urlencoded::to_string(reply).unwrap_or_default()
		);
		Response::builder()
			.status(StatusCode::SEE_OTHER)
			.header(header::LOCATION, location)
			.body(Full::default())
			.unwrap()
	}

	fn token(&self, params: &Params, authorization: Option<&str>) -> Res {
		let client_id =
			basic_auth_client_id(authorization).or_else(|| params.get("client_id").cloned());
		match params.get("grant_type").map(String::as_str) {
			Some("authorization_code") => {
				let code = params.get("code").map(String::as_str).unwrap_or("");
				let Some(grant) = self.codes.lock().unwrap().remove(code) else {
					return oauth_error("invalid_grant", "unknown or used code");
				};
				if grant.expires_at < unix_now() {
					return oauth_error("invalid_grant", "code expired");
				}
				if client_id.is_some_and(|c| c != grant.client_id) {
					return oauth_error("invalid_grant", "client_id mismatch");
				}
				if params.get("redirect_uri") != Some(&grant.redirect_uri) {
					return oauth_error("invalid_grant", "redirect_uri mismatch");
				}
				if let Some(challenge) = &grant.code_challenge {
					let verifier = params
						.get("code_verifier")
						.map(String::as_str)
						.unwrap_or("");
					if B64.encode(Sha256::digest(verifier)) != *challenge {
						return oauth_error("invalid_grant", "PKCE verification failed");
					}
				}
				json_response(
					StatusCode::OK,
					self.tokens(&grant.client_id, &grant.username, grant.nonce.as_deref()),
				)
			}
			Some("refresh_token") => {
				let token = params
					.get("refresh_token")
					.map(String::as_str)
					.unwrap_or("");
				let Some(claims) = self.verify(token) else {
					return oauth_error("invalid_grant", "invalid refresh token");
				};
				let (Some("refresh"), Some(aud), Some(username)) = (
					claims["token_use"].as_str(),
					claims["aud"].as_str(),
					claims["username"].as_str(),
				) else {
					return oauth_error("invalid_grant", "not a refresh token");
				};
				if client_id.is_some_and(|c| c != aud) {
					return oauth_error("invalid_grant", "client_id mismatch");
				}
				json_response(StatusCode::OK, self.tokens(aud, username, None))
			}
			_ => oauth_error(
				"unsupported_grant_type",
				"use authorization_code or refresh_token",
			),
		}
	}
}

async fn handle(provider: Arc<Provider>, req: Request<Incoming>) -> Result<Res, Infallible> {
	let method = req.method().clone();
	// Clients join the issuer (trailing `/`) and a path, which can give `//`.
	let path = req.uri().path().trim_start_matches('/').to_owned();
	let query = parse_form(req.uri().query().unwrap_or("").as_bytes());
	let authorization = req
		.headers()
		.get(header::AUTHORIZATION)
		.and_then(|v| v.to_str().ok())
		.map(str::to_owned);
	let body = req
		.into_body()
		.collect()
		.await
		.map(|b| b.to_bytes())
		.unwrap_or_default();

	let res = match (&method, path.as_str()) {
		(&Method::GET, ".well-known/openid-configuration") => {
			json_response(StatusCode::OK, provider.discovery())
		}
		(&Method::GET, "jwks") => json_response(StatusCode::OK, json!({ "keys": [provider.jwk] })),
		(&Method::GET, "authorize") => provider.login_page(&query),
		(&Method::POST, "authorize") => provider.authorize(&parse_form(&body)),
		(&Method::POST, "token") => provider.token(&parse_form(&body), authorization.as_deref()),
		_ => text(StatusCode::NOT_FOUND, "not found"),
	};
	eprintln!("dev-oidc: {method} /{path} -> {}", res.status().as_u16());
	Ok(res)
}

fn check_authorize_params(params: &Params) -> Result<(), &'static str> {
	if params.get("response_type").map(String::as_str) != Some("code") {
		return Err("response_type must be code");
	}
	if params.get("client_id").is_none_or(|c| c.is_empty()) {
		return Err("client_id is required");
	}
	let redirect = params.get("redirect_uri").map(String::as_str).unwrap_or("");
	if !(redirect.starts_with("http://") || redirect.starts_with("https://")) {
		return Err("redirect_uri must be an absolute http(s) URL");
	}
	Ok(())
}

/// Client id from `Authorization: Basic base64(urlencode(id):urlencode(secret))`.
/// The secret is not checked.
fn basic_auth_client_id(authorization: Option<&str>) -> Option<String> {
	let encoded = authorization?.strip_prefix("Basic ")?;
	let decoded = String::from_utf8(STANDARD.decode(encoded.trim()).ok()?).ok()?;
	let (id, _secret) = decoded.split_once(':')?;
	let pairs: Vec<(String, String)> = serde_urlencoded::from_str(&format!("id={id}")).ok()?;
	pairs.into_iter().next().map(|(_, v)| v)
}

/// Stable numeric BitCraft-style player id (the JWT `sub`) for a username.
/// Case-insensitive, and 53 bits so it stays exact as a JS number.
fn player_id(username: &str) -> u64 {
	let digest = Sha256::digest(format!("dev-oidc:{}", username.to_ascii_uppercase()));
	u64::from_be_bytes(digest[..8].try_into().unwrap()) >> 11
}

fn random_token() -> String {
	let mut bytes = [0u8; 32];
	OsRng.fill_bytes(&mut bytes);
	B64.encode(bytes)
}

fn unix_now() -> u64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|d| d.as_secs())
		.unwrap_or(0)
}

fn parse_form(bytes: &[u8]) -> Params {
	serde_urlencoded::from_bytes::<Vec<(String, String)>>(bytes)
		.unwrap_or_default()
		.into_iter()
		.collect()
}

fn escape(s: &str) -> String {
	s.replace('&', "&amp;")
		.replace('<', "&lt;")
		.replace('>', "&gt;")
		.replace('"', "&quot;")
		.replace('\'', "&#39;")
}

fn json_response(status: StatusCode, body: Value) -> Res {
	Response::builder()
		.status(status)
		.header(header::CONTENT_TYPE, "application/json")
		.header(header::CACHE_CONTROL, "no-store")
		.body(Full::new(Bytes::from(body.to_string())))
		.unwrap()
}

fn oauth_error(error: &str, description: &str) -> Res {
	eprintln!("dev-oidc: token error {error}: {description}");
	json_response(
		StatusCode::BAD_REQUEST,
		json!({ "error": error, "error_description": description }),
	)
}

fn html(body: String) -> Res {
	Response::builder()
		.header(header::CONTENT_TYPE, "text/html; charset=utf-8")
		.body(Full::new(Bytes::from(body)))
		.unwrap()
}

fn text(status: StatusCode, body: &'static str) -> Res {
	Response::builder()
		.status(status)
		.header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
		.body(Full::new(Bytes::from_static(body.as_bytes())))
		.unwrap()
}

#[cfg(test)]
mod tests {
	use super::*;

	fn provider() -> Provider {
		provider_at(DEFAULT_ISSUER)
	}

	#[test]
	fn player_id_is_stable_and_case_insensitive() {
		assert_eq!(player_id("TestFriend"), player_id("testfriend"));
		assert_ne!(player_id("alice"), player_id("bob"));
		assert!(player_id("alice") < 1 << 53);
	}

	#[test]
	fn signed_tokens_verify_and_tampering_fails() {
		let p = provider();
		let token = p.sign(&json!({ "sub": "1" }));
		assert_eq!(p.verify(&token).unwrap()["sub"], "1");
		let (head, _) = token.rsplit_once('.').unwrap();
		let forged = format!("{head}.{}", B64.encode([0u8; 256]));
		assert!(p.verify(&forged).is_none());
	}

	#[test]
	fn code_flow_with_pkce_then_refresh() {
		let p = provider();
		let verifier = "a-long-enough-pkce-verifier-string-0123456789";
		let challenge = B64.encode(Sha256::digest(verifier));
		let params: Params = [
			("response_type", "code"),
			("client_id", "nintron-stelofinance"),
			("redirect_uri", "http://127.0.0.1:8080/callback"),
			("state", "s1"),
			("nonce", "n1"),
			("code_challenge", challenge.as_str()),
			("code_challenge_method", "S256"),
			("username", "TestFriend"),
		]
		.into_iter()
		.map(|(k, v)| (k.into(), v.into()))
		.collect();

		let res = p.authorize(&params);
		assert_eq!(res.status(), StatusCode::SEE_OTHER);
		let location = res.headers()[header::LOCATION].to_str().unwrap();
		let query = parse_form(location.split_once('?').unwrap().1.as_bytes());
		assert_eq!(query["state"], "s1");

		let exchange = |verifier: &str| {
			let body: Params = [
				("grant_type", "authorization_code"),
				("code", query["code"].as_str()),
				("redirect_uri", "http://127.0.0.1:8080/callback"),
				("code_verifier", verifier),
			]
			.into_iter()
			.map(|(k, v)| (k.into(), v.into()))
			.collect();
			p.token(
				&body,
				Some(&format!(
					"Basic {}",
					STANDARD.encode("nintron-stelofinance:x")
				)),
			)
		};
		// Codes are single use: a wrong verifier burns it.
		assert_eq!(exchange("wrong").status(), StatusCode::BAD_REQUEST);
		assert_eq!(exchange(verifier).status(), StatusCode::BAD_REQUEST);

		let res = p.authorize(&params);
		let location = res.headers()[header::LOCATION].to_str().unwrap().to_owned();
		let query = parse_form(location.split_once('?').unwrap().1.as_bytes());
		let body: Params = [
			("grant_type", "authorization_code"),
			("code", query["code"].as_str()),
			("redirect_uri", "http://127.0.0.1:8080/callback"),
			("code_verifier", verifier),
		]
		.into_iter()
		.map(|(k, v)| (k.into(), v.into()))
		.collect();
		let tokens = body_json(p.token(&body, None));
		let id = p.verify(tokens["id_token"].as_str().unwrap()).unwrap();
		assert_eq!(id["iss"], DEFAULT_ISSUER);
		assert_eq!(id["aud"], "nintron-stelofinance");
		assert_eq!(id["nonce"], "n1");
		assert_eq!(id["preferred_username"], "TestFriend");
		assert_eq!(id["sub"], player_id("TestFriend").to_string());

		let refresh: Params = [
			("grant_type".to_owned(), "refresh_token".to_owned()),
			(
				"refresh_token".to_owned(),
				tokens["refresh_token"].as_str().unwrap().to_owned(),
			),
		]
		.into_iter()
		.collect();
		let refreshed = body_json(p.token(&refresh, None));
		let id = p.verify(refreshed["id_token"].as_str().unwrap()).unwrap();
		assert_eq!(id["preferred_username"], "TestFriend");
		assert!(id.get("nonce").is_none());

		// An ID token is not a refresh token.
		let bogus: Params = [
			("grant_type".to_owned(), "refresh_token".to_owned()),
			(
				"refresh_token".to_owned(),
				tokens["id_token"].as_str().unwrap().to_owned(),
			),
		]
		.into_iter()
		.collect();
		assert_eq!(p.token(&bogus, None).status(), StatusCode::BAD_REQUEST);
	}

	fn body_json(res: Res) -> Value {
		assert_eq!(res.status(), StatusCode::OK);
		let bytes = tokio::runtime::Builder::new_current_thread()
			.build()
			.unwrap()
			.block_on(res.into_body().collect())
			.unwrap()
			.to_bytes();
		serde_json::from_slice(&bytes).unwrap()
	}

	/// The edge's client (`src/auth/oidc.rs`, `openidconnect` 4) against a
	/// running server: discovery, PKCE login, ID token checks, refresh.
	#[tokio::test]
	async fn edge_client_round_trip() {
		use openidconnect::{
			AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce,
			OAuth2TokenResponse, PkceCodeChallenge, RedirectUrl, RefreshToken, Scope,
			TokenResponse,
			core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
		};

		let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
		let issuer = format!("http://{}/", listener.local_addr().unwrap());
		let p = Arc::new(provider_at(&issuer));
		tokio::spawn(async move { serve(listener, p).await.unwrap() });

		let http = reqwest::ClientBuilder::new()
			.redirect(reqwest::redirect::Policy::none())
			.build()
			.unwrap();
		let metadata =
			CoreProviderMetadata::discover_async(IssuerUrl::new(issuer.clone()).unwrap(), &http)
				.await
				.unwrap();
		let client = CoreClient::from_provider_metadata(
			metadata,
			ClientId::new("nintron-stelofinance".into()),
			Some(ClientSecret::new("dev".into())),
		)
		.set_redirect_uri(RedirectUrl::new("http://127.0.0.1:8080/callback".into()).unwrap());

		let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
		let (url, csrf, nonce) = client
			.authorize_url(
				CoreAuthenticationFlow::AuthorizationCode,
				CsrfToken::new_random,
				Nonce::new_random,
			)
			.add_scope(Scope::new("profile".into()))
			.add_scope(Scope::new("offline_access".into()))
			.set_pkce_challenge(challenge)
			.url();

		// The browser: load the login page, then submit it.
		let page = http.get(url.clone()).send().await.unwrap();
		assert_eq!(page.status(), 200);
		let mut form: Vec<(String, String)> = url.query_pairs().into_owned().collect();
		form.push(("username".into(), "Test Friend".into()));
		let submit = http
			.post(format!("{issuer}authorize"))
			.form(&form)
			.send()
			.await
			.unwrap();
		assert_eq!(submit.status(), 303);
		let location = reqwest::Url::parse(submit.headers()["location"].to_str().unwrap()).unwrap();
		let reply: HashMap<_, _> = location.query_pairs().into_owned().collect();
		assert_eq!(&reply["state"], csrf.secret());

		let tokens = client
			.exchange_code(AuthorizationCode::new(reply["code"].clone()))
			.unwrap()
			.set_pkce_verifier(verifier)
			.request_async(&http)
			.await
			.unwrap();
		let claims = tokens
			.id_token()
			.unwrap()
			.claims(&client.id_token_verifier(), &nonce)
			.unwrap();
		assert_eq!(claims.issuer().as_str(), issuer);
		assert_eq!(
			claims.subject().as_str(),
			player_id("Test Friend").to_string()
		);
		assert_eq!(claims.preferred_username().unwrap().as_str(), "Test Friend");

		let refreshed = client
			.exchange_refresh_token(&RefreshToken::new(
				tokens.refresh_token().unwrap().secret().clone(),
			))
			.unwrap()
			.add_scope(Scope::new("openid".into()))
			.request_async(&http)
			.await
			.unwrap();
		refreshed
			.id_token()
			.unwrap()
			.claims(&client.id_token_verifier(), |_: Option<&Nonce>| Ok(()))
			.unwrap();
	}

	fn provider_at(issuer: &str) -> Provider {
		Provider::new(issuer.into(), RsaPrivateKey::new(&mut OsRng, 2048).unwrap())
	}
}
