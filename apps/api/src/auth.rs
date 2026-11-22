// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use std::{
    collections::HashSet,
    env,
    net::IpAddr,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use futures_util::StreamExt;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use reqwest::{redirect::Policy, Client, Url};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const MAX_BEARER_BYTES: usize = 8 * 1024;
const MAX_KEY_FILE_BYTES: u64 = 16 * 1024;
const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const ASSERTION_LIFETIME_SECONDS: u64 = 60;
const ASSERTION_TYPE: &str = "urn:ietf:params:oauth:client-assertion-type:jwt-bearer";

#[derive(Clone)]
pub struct Authenticator {
    issuer: Arc<str>,
    introspection_endpoint: Url,
    api_audience: Arc<str>,
    allowed_client_ids: Arc<HashSet<String>>,
    client_id: Arc<str>,
    key_id: Arc<str>,
    signing_key: Arc<EncodingKey>,
    http: Client,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedIdentity {
    pub issuer: String,
    pub subject: String,
    pub client_id: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthenticationError {
    Unauthorized,
    Unavailable,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClientAssertionClaims {
    iss: String,
    sub: String,
    aud: String,
    iat: u64,
    exp: u64,
    jti: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct IntrospectionResponse {
    active: bool,
    iss: Option<String>,
    sub: Option<String>,
    aud: Option<Audience>,
    exp: Option<u64>,
    nbf: Option<u64>,
    client_id: Option<String>,
    token_type: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ZitadelKeyFile {
    client_id: String,
    key_id: String,
    key: String,
}

impl Authenticator {
    pub fn from_env() -> Result<Option<Self>, String> {
        Self::from_env_with_prefix("POS_API_OIDC")
    }

    pub fn from_env_with_prefix(prefix: &str) -> Result<Option<Self>, String> {
        let issuer = env::var(format!("{prefix}_ISSUER")).ok();
        let endpoint = env::var(format!("{prefix}_INTROSPECTION_URL")).ok();
        let audience = env::var(format!("{prefix}_API_AUDIENCE")).ok();
        let allowed_clients = env::var(format!("{prefix}_ALLOWED_CLIENT_IDS")).ok();
        let key_file = env::var(format!("{prefix}_KEY_FILE")).ok();
        let values = [
            issuer.as_deref(),
            endpoint.as_deref(),
            audience.as_deref(),
            allowed_clients.as_deref(),
            key_file.as_deref(),
        ];

        if values.iter().all(Option::is_none) {
            return Ok(None);
        }
        let (Some(issuer), Some(endpoint), Some(audience), Some(allowed_clients), Some(key_file)) =
            (issuer, endpoint, audience, allowed_clients, key_file)
        else {
            return Err("OIDC introspection requires issuer, endpoint, API audience, allowed clients, and key file configuration.".to_owned());
        };
        Self::new(
            &issuer,
            &endpoint,
            &audience,
            &allowed_clients,
            PathBuf::from(key_file),
        )
        .map(Some)
    }

    pub fn api_audience(&self) -> &str {
        &self.api_audience
    }

    pub fn allows_client_id(&self, client_id: &str) -> bool {
        self.allowed_client_ids.contains(client_id)
    }

    fn new(
        issuer: &str,
        introspection_endpoint: &str,
        api_audience: &str,
        allowed_client_ids: &str,
        key_file_path: PathBuf,
    ) -> Result<Self, String> {
        let issuer_url = parse_trusted_url("POS_API_OIDC_ISSUER", issuer)?;
        if issuer_url.path() != "/"
            || issuer_url.query().is_some()
            || issuer_url.fragment().is_some()
        {
            return Err(
                "POS_API_OIDC_ISSUER must be an HTTPS origin without a path, query, or fragment."
                    .to_owned(),
            );
        }
        let introspection_endpoint =
            parse_trusted_url("POS_API_OIDC_INTROSPECTION_URL", introspection_endpoint)?;
        if issuer_url.origin() != introspection_endpoint.origin() {
            return Err(
                "POS_API_OIDC_INTROSPECTION_URL must use the configured issuer origin.".to_owned(),
            );
        }
        if introspection_endpoint.query().is_some() {
            return Err(
                "POS_API_OIDC_INTROSPECTION_URL must not contain a query string.".to_owned(),
            );
        }
        if api_audience.trim().is_empty() || api_audience.len() > 512 {
            return Err("POS_API_OIDC_API_AUDIENCE must contain 1 to 512 characters.".to_owned());
        }
        let allowed_client_ids = allowed_client_ids
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect::<HashSet<_>>();
        if allowed_client_ids.is_empty() || allowed_client_ids.iter().any(|id| id.len() > 256) {
            return Err("POS_API_OIDC_ALLOWED_CLIENT_IDS must contain at least one client ID of at most 256 characters.".to_owned());
        }

        let metadata = std::fs::metadata(&key_file_path)
            .map_err(|_| "POS_API_OIDC_KEY_FILE could not be read.".to_owned())?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_KEY_FILE_BYTES {
            return Err(
                "POS_API_OIDC_KEY_FILE must be a non-empty file no larger than 16 KiB.".to_owned(),
            );
        }
        let key_json = std::fs::read(&key_file_path)
            .map_err(|_| "POS_API_OIDC_KEY_FILE could not be read.".to_owned())?;
        if key_json.len() as u64 > MAX_KEY_FILE_BYTES {
            return Err("POS_API_OIDC_KEY_FILE must be no larger than 16 KiB.".to_owned());
        }
        let key_file: ZitadelKeyFile = serde_json::from_slice(&key_json).map_err(|_| {
            "POS_API_OIDC_KEY_FILE must be a valid ZITADEL application key JSON file.".to_owned()
        })?;
        if key_file.client_id.trim().is_empty() || key_file.key_id.trim().is_empty() {
            return Err("The ZITADEL key file must include a client ID and key ID.".to_owned());
        }
        let signing_key = EncodingKey::from_rsa_pem(key_file.key.as_bytes())
            .map_err(|_| "The ZITADEL key file must contain a valid RSA private key.".to_owned())?;
        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(4))
            .redirect(Policy::none())
            .build()
            .map_err(|_| "The OIDC HTTP client could not be initialized.".to_owned())?;

        Ok(Self {
            issuer: Arc::from(issuer_url.origin().ascii_serialization()),
            introspection_endpoint,
            api_audience: Arc::from(api_audience.trim()),
            allowed_client_ids: Arc::new(allowed_client_ids),
            client_id: Arc::from(key_file.client_id),
            key_id: Arc::from(key_file.key_id),
            signing_key: Arc::new(signing_key),
            http,
        })
    }

    pub fn bearer_token(headers: &axum::http::HeaderMap) -> Result<&str, AuthenticationError> {
        let value = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .ok_or(AuthenticationError::Unauthorized)?;
        let (scheme, token) = value
            .split_once(' ')
            .ok_or(AuthenticationError::Unauthorized)?;
        if !scheme.eq_ignore_ascii_case("Bearer")
            || token.is_empty()
            || token.len() > MAX_BEARER_BYTES
            || token.bytes().any(|byte| byte <= 0x20 || byte >= 0x7f)
        {
            return Err(AuthenticationError::Unauthorized);
        }
        Ok(token)
    }

    pub async fn introspect(
        &self,
        access_token: &str,
    ) -> Result<VerifiedIdentity, AuthenticationError> {
        if access_token.is_empty()
            || access_token.len() > MAX_BEARER_BYTES
            || access_token
                .bytes()
                .any(|byte| byte <= 0x20 || byte >= 0x7f)
        {
            return Err(AuthenticationError::Unauthorized);
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| AuthenticationError::Unavailable)?
            .as_secs();
        let assertion_claims = ClientAssertionClaims {
            iss: self.client_id.to_string(),
            sub: self.client_id.to_string(),
            aud: self.issuer.to_string(),
            iat: now,
            exp: now + ASSERTION_LIFETIME_SECONDS,
            jti: Uuid::new_v4().to_string(),
        };
        let mut assertion_header = Header::new(Algorithm::RS256);
        assertion_header.kid = Some(self.key_id.to_string());
        let client_assertion = encode(&assertion_header, &assertion_claims, &self.signing_key)
            .map_err(|_| AuthenticationError::Unavailable)?;

        let response = self
            .http
            .post(self.introspection_endpoint.clone())
            .form(&[
                ("token", access_token),
                ("client_id", &self.client_id),
                ("client_assertion_type", ASSERTION_TYPE),
                ("client_assertion", &client_assertion),
            ])
            .send()
            .await
            .map_err(|_| AuthenticationError::Unavailable)?;
        if !response.status().is_success() {
            return Err(AuthenticationError::Unavailable);
        }
        let content_type_is_json = response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value
                    .split(';')
                    .next()
                    .is_some_and(|mime| mime.trim() == "application/json")
            });
        if !content_type_is_json {
            return Err(AuthenticationError::Unavailable);
        }

        let mut stream = response.bytes_stream();
        let mut body = Vec::with_capacity(1024);
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| AuthenticationError::Unavailable)?;
            if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(AuthenticationError::Unavailable);
            }
            body.extend_from_slice(&chunk);
        }
        let response: IntrospectionResponse =
            serde_json::from_slice(&body).map_err(|_| AuthenticationError::Unavailable)?;
        self.validate_introspection(response, now)
    }

    fn validate_introspection(
        &self,
        response: IntrospectionResponse,
        now: u64,
    ) -> Result<VerifiedIdentity, AuthenticationError> {
        validate_introspection_response(
            response,
            now,
            &self.issuer,
            &self.api_audience,
            &self.allowed_client_ids,
        )
    }
}

fn validate_introspection_response(
    response: IntrospectionResponse,
    now: u64,
    expected_issuer: &str,
    expected_audience: &str,
    allowed_client_ids: &HashSet<String>,
) -> Result<VerifiedIdentity, AuthenticationError> {
    if !response.active {
        return Err(AuthenticationError::Unauthorized);
    }
    let issuer = response.iss.ok_or(AuthenticationError::Unauthorized)?;
    let subject = response.sub.ok_or(AuthenticationError::Unauthorized)?;
    let client_id = response
        .client_id
        .ok_or(AuthenticationError::Unauthorized)?;
    let expires_at = response.exp.ok_or(AuthenticationError::Unauthorized)?;
    let token_type = response
        .token_type
        .ok_or(AuthenticationError::Unauthorized)?;
    let audience_matches = response
        .aud
        .as_ref()
        .is_some_and(|audience| audience.contains(expected_audience));
    if issuer != expected_issuer
        || subject.trim().is_empty()
        || expires_at <= now
        || response.nbf.is_some_and(|not_before| not_before > now)
        || !audience_matches
        || !allowed_client_ids.contains(&client_id)
        || !token_type.eq_ignore_ascii_case("bearer")
    {
        return Err(AuthenticationError::Unauthorized);
    }
    Ok(VerifiedIdentity {
        issuer,
        subject,
        client_id,
        expires_at,
    })
}

impl Audience {
    fn contains(&self, expected: &str) -> bool {
        match self {
            Self::One(value) => value == expected,
            Self::Many(values) => values.iter().any(|value| value == expected),
        }
    }
}

fn parse_trusted_url(name: &str, value: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|_| format!("{name} must be a valid HTTPS URL."))?;
    let is_loopback_http = url.scheme() == "http"
        && url.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .parse::<IpAddr>()
                    .is_ok_and(|address| address.is_loopback())
        });
    if (url.scheme() != "https" && !is_loopback_http)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(format!(
            "{name} must use HTTPS; HTTP is allowed only for loopback development."
        ));
    }
    Ok(url)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{collections::HashMap, sync::Arc, time::Duration};

    use axum::{
        extract::{Form, State},
        http::StatusCode,
        response::IntoResponse,
        routing::post,
        Json, Router,
    };
    use jsonwebtoken::{decode, DecodingKey, Validation};
    use rand::rngs::OsRng;
    use rsa::{
        pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding},
        RsaPrivateKey,
    };
    use tokio::net::TcpListener;

    use super::*;

    fn valid_response(issuer: &str, client_id: &str, expires_at: u64) -> IntrospectionResponse {
        IntrospectionResponse {
            active: true,
            iss: Some(issuer.to_owned()),
            sub: Some("person-123".to_owned()),
            aud: Some(Audience::Many(vec![
                "other-api".to_owned(),
                "pos-api".to_owned(),
            ])),
            exp: Some(expires_at),
            nbf: Some(99),
            client_id: Some(client_id.to_owned()),
            token_type: Some("Bearer".to_owned()),
        }
    }

    #[test]
    fn verifies_only_active_tokens_for_the_configured_issuer_audience_and_client() {
        let allowed = HashSet::from(["web-client".to_owned(), "desktop-client".to_owned()]);
        let valid = valid_response("https://identity.example", "web-client", 200);
        let identity = validate_introspection_response(
            valid,
            100,
            "https://identity.example",
            "pos-api",
            &allowed,
        )
        .unwrap();
        assert_eq!(identity.subject, "person-123");
        assert_eq!(identity.expires_at, 200);

        let mut invalid_responses = Vec::new();
        let mut response = valid_response("https://identity.example", "web-client", 200);
        response.active = false;
        invalid_responses.push(response);
        let response = valid_response("https://other.example", "web-client", 200);
        invalid_responses.push(response);
        let response = valid_response("https://identity.example", "unknown-client", 200);
        invalid_responses.push(response);
        let response = valid_response("https://identity.example", "web-client", 100);
        invalid_responses.push(response);
        let mut response = valid_response("https://identity.example", "web-client", 200);
        response.nbf = Some(101);
        invalid_responses.push(response);
        let mut response = valid_response("https://identity.example", "web-client", 200);
        response.aud = Some(Audience::One("other-api".to_owned()));
        invalid_responses.push(response);
        for response in invalid_responses {
            assert_eq!(
                validate_introspection_response(
                    response,
                    100,
                    "https://identity.example",
                    "pos-api",
                    &allowed,
                ),
                Err(AuthenticationError::Unauthorized)
            );
        }
    }

    #[test]
    fn bearer_parser_rejects_missing_wrong_scheme_controls_and_oversized_values() {
        let mut headers = axum::http::HeaderMap::new();
        assert_eq!(
            Authenticator::bearer_token(&headers),
            Err(AuthenticationError::Unauthorized)
        );

        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Basic abc".parse().unwrap(),
        );
        assert_eq!(
            Authenticator::bearer_token(&headers),
            Err(AuthenticationError::Unauthorized)
        );

        headers.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", "a".repeat(MAX_BEARER_BYTES + 1))
                .parse()
                .unwrap(),
        );
        assert_eq!(
            Authenticator::bearer_token(&headers),
            Err(AuthenticationError::Unauthorized)
        );

        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer opaque-token".parse().unwrap(),
        );
        assert_eq!(Authenticator::bearer_token(&headers), Ok("opaque-token"));
    }

    #[test]
    fn trusted_endpoint_configuration_rejects_non_loopback_http_and_credentials() {
        assert!(parse_trusted_url("issuer", "https://identity.example").is_ok());
        assert!(parse_trusted_url("issuer", "http://127.0.0.1:9000").is_ok());
        assert!(parse_trusted_url("issuer", "http://identity.example").is_err());
        assert!(parse_trusted_url("issuer", "https://user:pass@identity.example").is_err());
        assert!(parse_trusted_url("issuer", "https://identity.example#fragment").is_err());
    }

    #[derive(Clone)]
    struct MockState {
        issuer: String,
        public_key_pem: Arc<String>,
        response: IntrospectionResponse,
        accepted_token: String,
    }

    async fn mock_introspect(
        State(state): State<MockState>,
        Form(form): Form<HashMap<String, String>>,
    ) -> impl IntoResponse {
        let assertion = form.get("client_assertion").expect("client assertion sent");
        let key = DecodingKey::from_rsa_pem(state.public_key_pem.as_bytes()).unwrap();
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&["test-api-client"]);
        validation.set_audience(&[&state.issuer]);
        let assertion = decode::<ClientAssertionClaims>(assertion, &key, &validation).unwrap();
        assert_eq!(assertion.claims.iss, "test-api-client");
        assert_eq!(assertion.claims.sub, "test-api-client");
        assert_eq!(assertion.claims.aud, state.issuer);
        assert_eq!(assertion.header.kid.as_deref(), Some("test-key-id"));
        let mut response = state.response;
        if form.get("token") != Some(&state.accepted_token) {
            response.active = false;
        }
        assert_eq!(
            form.get("client_id").map(String::as_str),
            Some("test-api-client")
        );
        assert_eq!(
            form.get("client_assertion_type").map(String::as_str),
            Some(ASSERTION_TYPE)
        );
        (StatusCode::OK, Json(response)).into_response()
    }

    pub(crate) async fn start_test_authenticator(
        subject: &str,
    ) -> (String, Authenticator, tokio::task::JoinHandle<()>) {
        start_test_authenticator_for(subject, "opaque-token", "pos-api", "cashier-app", u64::MAX)
            .await
    }

    pub(crate) async fn start_test_authenticator_for(
        subject: &str,
        token: &str,
        audience: &str,
        allowed_client: &str,
        lifetime_seconds: u64,
    ) -> (String, Authenticator, tokio::task::JoinHandle<()>) {
        let private_key = RsaPrivateKey::new(&mut OsRng, 2048).unwrap();
        let private_pem = private_key.to_pkcs8_pem(LineEnding::LF).unwrap();
        let public_pem = private_key
            .to_public_key()
            .to_public_key_pem(LineEnding::LF)
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let issuer = format!("http://{address}");
        let expiration = if lifetime_seconds == u64::MAX {
            u64::MAX
        } else {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                .saturating_add(lifetime_seconds)
        };
        let mut response = valid_response(&issuer, allowed_client, expiration);
        response.sub = Some(subject.to_owned());
        response.aud = Some(Audience::One(audience.to_owned()));
        let state = MockState {
            issuer: issuer.clone(),
            public_key_pem: Arc::new(public_pem),
            response,
            accepted_token: token.to_owned(),
        };
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/introspect", post(mock_introspect))
                    .with_state(state),
            )
            .await
            .unwrap();
        });
        let authenticator = Authenticator {
            issuer: Arc::from(issuer.as_str()),
            introspection_endpoint: Url::parse(&format!("{issuer}/introspect")).unwrap(),
            api_audience: Arc::from(audience),
            allowed_client_ids: Arc::new(HashSet::from([allowed_client.to_owned()])),
            client_id: Arc::from("test-api-client"),
            key_id: Arc::from("test-key-id"),
            signing_key: Arc::new(EncodingKey::from_rsa_pem(private_pem.as_bytes()).unwrap()),
            http: Client::builder()
                .timeout(Duration::from_secs(2))
                .redirect(Policy::none())
                .build()
                .unwrap(),
        };
        (issuer, authenticator, server)
    }

    #[tokio::test]
    async fn introspects_opaque_token_with_a_short_lived_private_key_jwt_assertion() {
        let (_, authenticator, server) = start_test_authenticator("person-123").await;
        let identity = authenticator.introspect("opaque-token").await.unwrap();
        assert_eq!(identity.subject, "person-123");
        assert_eq!(identity.client_id, "cashier-app");
        server.abort();
    }
}
