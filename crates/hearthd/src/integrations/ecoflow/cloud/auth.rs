//! Getting from an EcoFlow account email and password to MQTT credentials.
//!
//! Two calls, both against the consumer ("private") API host, which defaults
//! to `api.ecoflow.com`:
//!
//! ```text
//! 1. POST /auth/login
//!    email + base64(password)        ->  bearer token, userId
//!
//! 2. GET  /iot-auth/app/certification
//!    Authorization: Bearer {token}   ->  MQTT host, port, username, password
//! ```
//!
//! Both run once at startup and again whenever the broker rejects the stored
//! credentials.
//!
//! This is not the public developer API. The hosts `api-e.ecoflow.com` and
//! `api-a.ecoflow.com`, the `/iot-open/...` paths and access-key/secret-key
//! signing belong to a different protocol that does not expose the Wave 3.
//!
//! # Response envelope
//!
//! Every endpoint returns the same envelope with a top-level `message`. A call
//! succeeded only when the HTTP status is 200 *and* `message` equals
//! `"success"`, compared case-insensitively. Anything else is a failure whose
//! reason is the value of `message`. A missing `message` is itself an error —
//! success is never assumed.
//!
//! # Secrets
//!
//! The account password grants full control of every device on the account,
//! including firmware-level settings. The base64 in the login body is
//! obfuscation, not a hash: it is trivially reversible and must be treated
//! exactly as sensitively as the plaintext. Neither it, the bearer token, nor
//! the MQTT credentials are ever logged, and TLS certificate verification is
//! mandatory because all three are bearer-style secrets.
//!
//! # Attribution
//!
//! EcoFlow does not publish this handshake. The two-step login-then-
//! certification flow, the exact request bodies and headers, and the
//! `message == "success"` envelope convention were reverse-engineered by the
//! `tolwi/hassio-ecoflow-cloud` Home Assistant custom component (Apache-2.0),
//! read at commit `a7ebbba`, in
//! `custom_components/ecoflow_cloud/api/private_api.py` and
//! `custom_components/ecoflow_cloud/api/__init__.py`.
//!
//! What that project provided is knowledge of the request shapes. The Rust
//! below, its error handling and its tests are original to hearthd. No code
//! was copied.

use std::fmt;

use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Deserialize;

/// Default private-API host. Not region-selected by the client: EcoFlow routes
/// by account.
pub const DEFAULT_API_HOST: &str = "api.ecoflow.com";

const LOGIN_PATH: &str = "/auth/login";
const CERTIFICATION_PATH: &str = "/iot-auth/app/certification";

/// What a successful login yields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// Bearer token for the certification call.
    pub token: String,
    /// Account identifier, used in the MQTT client id and every per-device
    /// command topic. Opaque: numeric-looking but treated as a string.
    pub user_id: String,
}

/// Per-user MQTT credentials. Reissued on every certification call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MqttCredentials {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

#[derive(Debug)]
pub enum AuthError {
    /// The transport failed: DNS, TLS, connection reset.
    Transport(String),
    /// The endpoint answered, but not with success. Carries the `message`.
    Rejected(String),
    /// The response was HTTP 200 and nominally successful but structurally
    /// wrong.
    Malformed(String),
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuthError::Transport(e) => write!(f, "EcoFlow API request failed: {e}"),
            AuthError::Rejected(m) => write!(f, "EcoFlow API rejected the request: {m}"),
            AuthError::Malformed(e) => write!(f, "EcoFlow API response was malformed: {e}"),
        }
    }
}

impl std::error::Error for AuthError {}

/// The two private-API calls, behind a trait so the integration can be tested
/// without a network.
#[async_trait]
pub trait EcoFlowApi: Send + Sync {
    async fn login(&self, email: &str, password: &str) -> Result<Session, AuthError>;
    async fn certification(&self, token: &str) -> Result<MqttCredentials, AuthError>;
}

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    message: Option<String>,
    data: Option<T>,
}

#[derive(Debug, Deserialize)]
struct LoginData {
    token: Option<String>,
    user: Option<LoginUser>,
}

#[derive(Debug, Deserialize)]
struct LoginUser {
    #[serde(rename = "userId")]
    user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CertificationData {
    url: Option<String>,
    /// Arrives as a string, not a number.
    port: Option<String>,
    #[serde(rename = "certificateAccount")]
    certificate_account: Option<String>,
    #[serde(rename = "certificatePassword")]
    certificate_password: Option<String>,
}

/// Apply the envelope rule and unwrap `data`.
///
/// `status_ok` is whether the HTTP status was 200. Both conditions must hold;
/// neither alone is sufficient, and an absent `message` is a failure rather
/// than an implied success.
fn unwrap_envelope<T>(status_ok: bool, body: &str) -> Result<T, AuthError>
where
    T: for<'de> Deserialize<'de>,
{
    let envelope: Envelope<T> = serde_json::from_str(body)
        .map_err(|e| AuthError::Malformed(format!("could not parse response body: {e}")))?;

    let message = envelope
        .message
        .ok_or_else(|| AuthError::Malformed("response has no message field".to_string()))?;

    if !status_ok || !message.eq_ignore_ascii_case("success") {
        return Err(AuthError::Rejected(message));
    }

    envelope
        .data
        .ok_or_else(|| AuthError::Malformed("successful response has no data".to_string()))
}

fn parse_login(status_ok: bool, body: &str) -> Result<Session, AuthError> {
    let data: LoginData = unwrap_envelope(status_ok, body)?;

    let token = data
        .token
        .ok_or_else(|| AuthError::Malformed("login response has no token".to_string()))?;
    let user_id = data
        .user
        .and_then(|u| u.user_id)
        .ok_or_else(|| AuthError::Malformed("login response has no userId".to_string()))?;

    Ok(Session { token, user_id })
}

fn parse_certification(status_ok: bool, body: &str) -> Result<MqttCredentials, AuthError> {
    let data: CertificationData = unwrap_envelope(status_ok, body)?;

    let host = data
        .url
        .ok_or_else(|| AuthError::Malformed("certification response has no url".to_string()))?;
    let port = data
        .port
        .ok_or_else(|| AuthError::Malformed("certification response has no port".to_string()))?
        .parse::<u16>()
        .map_err(|e| AuthError::Malformed(format!("certification port is not a number: {e}")))?;
    let username = data.certificate_account.ok_or_else(|| {
        AuthError::Malformed("certification response has no certificateAccount".to_string())
    })?;
    let password = data.certificate_password.ok_or_else(|| {
        AuthError::Malformed("certification response has no certificatePassword".to_string())
    })?;

    Ok(MqttCredentials {
        host,
        port,
        username,
        password,
    })
}

/// Encode a password for the login body.
///
/// Standard alphabet with padding. This is obfuscation, not encryption — TLS
/// is what protects it.
fn encode_password(password: &str) -> String {
    BASE64.encode(password.as_bytes())
}

/// The real client, talking to the private API over HTTPS.
pub struct HttpApi {
    client: reqwest::Client,
    host: String,
}

impl HttpApi {
    pub fn new(host: impl Into<String>) -> Result<Self, AuthError> {
        // The same configuration the MQTT transport uses. Left to itself
        // reqwest builds one from the host's trust store, which fails outright
        // wherever there isn't one — a build sandbox, a minimal container —
        // and would have the two connections trusting different roots.
        let tls = super::tls::client_config().map_err(AuthError::Transport)?;

        let client = reqwest::Client::builder()
            .use_preconfigured_tls((*tls).clone())
            .https_only(true)
            .build()
            .map_err(|e| AuthError::Transport(e.to_string()))?;

        Ok(Self {
            client,
            host: host.into(),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("https://{}{}", self.host, path)
    }
}

#[async_trait]
impl EcoFlowApi for HttpApi {
    async fn login(&self, email: &str, password: &str) -> Result<Session, AuthError> {
        let body = serde_json::json!({
            "email": email,
            "password": encode_password(password),
            "scene": "IOT_APP",
            "userType": "ECOFLOW",
        });

        let response = self
            .client
            .post(self.url(LOGIN_PATH))
            .header("lang", "en_US")
            .json(&body)
            .send()
            .await
            .map_err(|e| AuthError::Transport(e.to_string()))?;

        let status_ok = response.status().is_success();
        let text = response
            .text()
            .await
            .map_err(|e| AuthError::Transport(e.to_string()))?;

        parse_login(status_ok, &text)
    }

    async fn certification(&self, token: &str) -> Result<MqttCredentials, AuthError> {
        // Sent without a body. The upstream client attaches a urlencoded
        // userId to this GET while declaring a JSON content type; the bearer
        // token already identifies the user, and a body on a GET is unusual.
        // If the endpoint ever rejects this, that body is the first thing to
        // try adding back.
        let response = self
            .client
            .get(self.url(CERTIFICATION_PATH))
            .header("lang", "en_US")
            .header("authorization", format!("Bearer {token}"))
            .send()
            .await
            .map_err(|e| AuthError::Transport(e.to_string()))?;

        let status_ok = response.status().is_success();
        let text = response
            .text()
            .await
            .map_err(|e| AuthError::Transport(e.to_string()))?;

        parse_certification(status_ok, &text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOGIN_OK: &str = r#"{
        "message": "Success",
        "data": { "token": "abc123", "user": { "userId": "1234567890", "name": "Jo" } }
    }"#;

    const CERT_OK: &str = r#"{
        "message": "Success",
        "data": {
            "url": "mqtt.ecoflow.com",
            "port": "8883",
            "protocol": "mqtts",
            "certificateAccount": "user-abc",
            "certificatePassword": "pass-def"
        }
    }"#;

    #[test]
    fn the_real_client_can_actually_be_built() {
        // Every other test here drives parsing directly or uses a mock, so
        // nothing else exercises reqwest's builder. Two separate failures got
        // through before this existed: a panic when no process-wide crypto
        // provider was installed, and an error wherever the host has no trust
        // store. Both only ever showed up at runtime.
        assert!(HttpApi::new("api.example.com").is_ok());
    }

    #[test]
    fn password_is_base64_with_padding() {
        assert_eq!(encode_password("hunter2"), "aHVudGVyMg==");
        assert_eq!(encode_password(""), "");
    }

    #[test]
    fn successful_login_yields_token_and_user_id() {
        let session = parse_login(true, LOGIN_OK).unwrap();
        assert_eq!(session.token, "abc123");
        assert_eq!(session.user_id, "1234567890");
    }

    #[test]
    fn successful_certification_parses_the_string_port() {
        let creds = parse_certification(true, CERT_OK).unwrap();
        assert_eq!(creds.host, "mqtt.ecoflow.com");
        assert_eq!(creds.port, 8883);
        assert_eq!(creds.username, "user-abc");
        assert_eq!(creds.password, "pass-def");
    }

    #[test]
    fn message_is_compared_case_insensitively() {
        for message in ["Success", "success", "SUCCESS"] {
            let body = LOGIN_OK.replace("\"Success\"", &format!("\"{message}\""));
            assert!(parse_login(true, &body).is_ok(), "message {message}");
        }
    }

    #[test]
    fn a_non_success_message_is_rejected_with_its_reason() {
        let body = r#"{"message": "Invalid email or password"}"#;
        match parse_login(true, body) {
            Err(AuthError::Rejected(m)) => assert_eq!(m, "Invalid email or password"),
            other => panic!("expected rejection, got {other:?}"),
        }
    }

    #[test]
    fn a_non_200_status_is_rejected_even_when_the_message_says_success() {
        // Both conditions must hold; neither alone is sufficient.
        match parse_login(false, LOGIN_OK) {
            Err(AuthError::Rejected(m)) => assert_eq!(m, "Success"),
            other => panic!("expected rejection, got {other:?}"),
        }
    }

    #[test]
    fn a_missing_message_is_an_error_rather_than_implied_success() {
        let body = r#"{"data": {"token": "abc", "user": {"userId": "1"}}}"#;
        assert!(matches!(
            parse_login(true, body),
            Err(AuthError::Malformed(_))
        ));
    }

    #[test]
    fn a_success_without_data_is_malformed() {
        assert!(matches!(
            parse_login(true, r#"{"message": "Success"}"#),
            Err(AuthError::Malformed(_))
        ));
    }

    #[test]
    fn missing_login_fields_are_reported_individually() {
        let no_token = r#"{"message":"Success","data":{"user":{"userId":"1"}}}"#;
        let no_user = r#"{"message":"Success","data":{"token":"abc"}}"#;
        assert!(matches!(
            parse_login(true, no_token),
            Err(AuthError::Malformed(_))
        ));
        assert!(matches!(
            parse_login(true, no_user),
            Err(AuthError::Malformed(_))
        ));
    }

    #[test]
    fn a_non_numeric_port_is_malformed() {
        let body = CERT_OK.replace("\"8883\"", "\"not-a-port\"");
        assert!(matches!(
            parse_certification(true, &body),
            Err(AuthError::Malformed(_))
        ));
    }

    #[test]
    fn a_garbage_body_is_malformed_not_a_panic() {
        assert!(matches!(
            parse_login(true, "<html>502 Bad Gateway</html>"),
            Err(AuthError::Malformed(_))
        ));
    }

    #[test]
    fn errors_never_contain_the_credentials() {
        // A rejection carries only the server's message, so a password can
        // never reach a log through this path.
        let body = r#"{"message": "Invalid email or password"}"#;
        let rendered = parse_login(true, body).unwrap_err().to_string();
        assert!(!rendered.contains("hunter2"));
        assert!(!rendered.contains("aHVudGVyMg"));
    }
}
