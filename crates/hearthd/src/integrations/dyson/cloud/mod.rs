//! Dyson cloud API client.
//!
//! The cloud API is only used once, to bootstrap local MQTT credentials. The
//! flow is:
//!
//! 1. `DysonAccount::request_email_otp(email, region)` triggers an email OTP.
//! 2. The user supplies the OTP and their account password.
//! 3. `DysonAccount::verify_email_otp(...)` returns auth info.
//! 4. `DysonAccount::devices()` fetches the device manifest.
//!
//! After that, the daemon never talks to the cloud again.

use std::collections::HashMap;

use anyhow::Context;
use anyhow::Result;
use reqwest::Client;
use reqwest::Method;

pub const DEFAULT_API_HOST: &str = "https://appapi.cp.dyson.com";
static USER_AGENT: &str = "android client";

/// Two-letter country/region codes accepted by the Dyson Dyson email-OTP
/// login flow, as defined by `libdyson.cloud.regions.REGIONS`.
pub const ACCOUNT_REGIONS: &[&str] = &[
    "AU", // Australia
    "AT", // Austria
    "BE", // Belgium
    "CA", // Canada
    "HR", // Croatia
    "CZ", // Czechia
    "DK", // Denmark
    "FI", // Finland
    "FR", // France
    "DE", // Germany
    "HK", // Hong Kong
    "HU", // Hungary
    "IN", // India
    "ID", // Indonesia
    "IE", // Ireland
    "IL", // Israel
    "IT", // Italy
    "JP", // Japan
    "LT", // Lithuania
    "CN", // Mainland China
    "MY", // Malaysia
    "MX", // Mexico
    "NL", // Netherlands
    "NZ", // New Zealand
    "NO", // Norway
    "PH", // Philippines
    "PL", // Poland
    "PT", // Portugal
    "RO", // Romania
    "SA", // Saudi Arabia
    "SG", // Singapore
    "SI", // Slovenia
    "KR", // South Korea
    "ES", // Spain
    "SE", // Sweden
    "CH", // Switzerland
    "TW", // Taiwan
    "TH", // Thailand
    "TR", // Turkey
    "AE", // United Arab Emirates
    "GB", // United Kingdom
    "US", // United States of America
];

/// A Dyson account session used to fetch the local device credentials.
pub struct DysonAccount {
    client: Client,
    host: String,
    auth_info: Option<serde_json::Value>,
    challenge_id: Option<String>,
}

impl DysonAccount {
    pub fn new() -> Result<Self> {
        Self::with_host(DEFAULT_API_HOST.to_string())
    }

    pub fn with_host(host: String) -> Result<Self> {
        let tls = crate::tls::client_config().map_err(anyhow::Error::msg)?;
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .use_preconfigured_tls((*tls).clone())
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self {
            client,
            host,
            auth_info: None,
            challenge_id: None,
        })
    }

    /// Request an email OTP. Returns nothing; the OTP is sent by Dyson.
    pub async fn request_email_otp(&mut self, email: &str, region: &str) -> Result<()> {
        // Request an OTP for the account. (libdyson first hits an
        // `/userstatus` endpoint to reject unknown accounts with a nicer
        // message, but that call is unauthenticated, intermittently flaky, and
        // not actually part of the login flow — so we skip it and go straight
        // to requesting the OTP, which is what matters.)
        let auth_path = "/v3/userregistration/email/auth";
        let response: serde_json::Value = self
            .request(
                Method::POST,
                auth_path,
                Some(
                    &[("country", region), ("culture", "en-US")]
                        .into_iter()
                        .collect(),
                ),
                Some(&serde_json::json!({ "email": email })),
                false,
            )
            .await
            .context("failed to request email OTP")?;

        let challenge_id = response["challengeId"]
            .as_str()
            .context("challengeId missing from OTP response")?
            .to_string();
        self.challenge_id = Some(challenge_id);
        Ok(())
    }

    /// Verify the OTP and password, completing login.
    pub async fn verify_email_otp(
        &mut self,
        email: &str,
        password: &str,
        otp_code: &str,
    ) -> Result<()> {
        let challenge_id = self
            .challenge_id
            .take()
            .context("no OTP challenge in progress")?;
        let verify_path = "/v3/userregistration/email/verify";
        let body = serde_json::json!({
            "email": email,
            "password": password,
            "challengeId": challenge_id,
            "otpCode": otp_code,
        });
        let response: serde_json::Value = self
            .request(Method::POST, verify_path, None, Some(&body), false)
            .await
            .context("failed to verify email OTP")?;
        self.auth_info = Some(response);
        Ok(())
    }

    /// Fetch the device manifest from the cloud account.
    pub async fn devices(&self) -> Result<Vec<DeviceInfo>> {
        let manifest: Vec<serde_json::Value> = self
            .request(
                Method::GET,
                "/v2/provisioningservice/manifest",
                None,
                None,
                true,
            )
            .await
            .context("failed to fetch device manifest")?;

        manifest
            .into_iter()
            .filter_map(|raw| {
                // Lightcycle lights do not have LocalCredentials and are not
                // supported by this integration.
                if raw.get("LocalCredentials").is_some() {
                    Some(DeviceInfo::from_raw(raw))
                } else {
                    None
                }
            })
            .collect::<Result<Vec<_>>>()
    }

    async fn request<T: serde::de::DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        params: Option<&HashMap<&str, &str>>,
        body: Option<&serde_json::Value>,
        auth: bool,
    ) -> Result<T> {
        let mut request = self
            .client
            .request(method, format!("{}{}", self.host, path))
            .header("User-Agent", USER_AGENT);

        if let Some(params) = params {
            request = request.query(params);
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        if auth {
            let token = self
                .auth_info
                .as_ref()
                .and_then(|info| info.get("token").and_then(|t| t.as_str()))
                .context("not authenticated")?;
            request = request.bearer_auth(token);
        }

        let response = request.send().await.context("HTTP request failed")?;
        let status = response.status();
        if !status.is_success() {
            // Surface the real Dyson status and body: the "unauthenticated"
            // first call returning 401/403 often means something specific (e.g.
            // an unknown account or a changed API), and a guess like "invalid
            // credentials" hides it.
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable body>".to_string());
            anyhow::bail!("Dyson API {status} for {path}: {body}", path = path);
        }
        response
            .json::<T>()
            .await
            .context("failed to parse Dyson response")
    }
}

/// Information about one Dyson device, as returned by the cloud API.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub serial: String,
    pub credential: String,
    pub device_type: String,
    pub name: String,
}

impl DeviceInfo {
    fn from_raw(raw: serde_json::Value) -> Result<Self> {
        let serial = raw["Serial"]
            .as_str()
            .context("manifest entry missing Serial")?
            .to_string();
        let device_type = raw["ProductType"]
            .as_str()
            .context("manifest entry missing ProductType")?
            .to_string();
        let name = raw["Name"].as_str().unwrap_or("Dyson Device").to_string();

        // `LocalCredentials` is not a plain object: it is the MQTT password,
        // AES-256-CBC encrypted with Dyson's fixed key/IV and base64 encoded.
        // Decrypting yields a JSON blob whose `apPasswordHash` is the password
        // the device's local MQTT broker accepts.
        let credential = raw["LocalCredentials"]
            .as_str()
            .context("manifest entry missing LocalCredentials")?;
        let credential = decrypt_credential(credential)?;

        Ok(Self {
            serial,
            credential,
            device_type,
            name,
        })
    }
}

/// Dyson's fixed AES key (0x01..=0x20) and all-zero IV, shared by every
/// device and embedded in the official app / libdyson.
const DYSON_ENCRYPTION_KEY: [u8; 32] = {
    let mut key = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        key[i] = (i as u8) + 1;
        i += 1;
    }
    key
};
const DYSON_ENCRYPTION_IV: [u8; 16] = [0u8; 16];

/// Decrypt a `LocalCredentials` value into the local MQTT password.
fn decrypt_credential(encrypted: &str) -> Result<String> {
    use aes::cipher::BlockDecryptMut;
    use aes::cipher::KeyIvInit;
    use aes::cipher::block_padding::Pkcs7;
    use base64::Engine;

    let ciphertext = base64::engine::general_purpose::STANDARD
        .decode(encrypted)
        .context("LocalCredentials is not valid base64")?;
    let mut buf = ciphertext;

    let decryptor =
        cbc::Decryptor::<aes::Aes256>::new_from_slices(&DYSON_ENCRYPTION_KEY, &DYSON_ENCRYPTION_IV)
            .map_err(|e| anyhow::anyhow!("failed to initialise decryption cipher: {e}"))?;
    let decrypted = decryptor
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|e| anyhow::anyhow!("failed to decrypt LocalCredentials: {e}"))?;

    let json: serde_json::Value = serde_json::from_slice(decrypted)
        .context("decrypted LocalCredentials is not valid JSON")?;
    Ok(json["apPasswordHash"]
        .as_str()
        .context("decrypted LocalCredentials has no apPasswordHash")?
        .to_string())
}

#[cfg(test)]
mod tests {
    use base64::Engine;

    use super::*;

    /// Plaintext matching libdyson's `tests/cloud/test_dyson_account.py`
    /// `DEVICE1`, encrypted by Dyson's own `encrypt_credential`.
    const DEVICE1_SERIAL: &str = "NK6-CN-HAA0000A";
    const DEVICE1_CREDENTIAL: &str =
        "aoWJM1kpL79MN2dPMlL5ysQv/APG+HAv+x3HDk0yuT3gMfgA3mLuil4O3d+q6CcyU+D1Hoir38soKoZHshYFeQ==";

    /// Encrypt the plaintext the same way libdyson's `encrypt_credential`
    /// does, so the test can confirm our decrypt reverses it (and that the
    /// key/IV/padding all match Dyson's fixed scheme).
    fn encrypt_credential(serial: &str, credential: &str) -> String {
        use aes::cipher::BlockEncryptMut;
        use aes::cipher::KeyIvInit;
        use aes::cipher::block_padding::Pkcs7;

        let plaintext = serde_json::json!({
            "serial": serial,
            "apPasswordHash": credential,
        })
        .to_string()
        .into_bytes();

        // The output buffer must hold the plaintext plus one full PKCS7
        // padding block.
        let mut out = vec![0u8; plaintext.len() + 16];
        let enc = cbc::Encryptor::<aes::Aes256>::new_from_slices(
            &DYSON_ENCRYPTION_KEY,
            &DYSON_ENCRYPTION_IV,
        )
        .expect("valid key/iv");
        let len = enc
            .encrypt_padded_b2b_mut::<Pkcs7>(&plaintext, &mut out)
            .expect("output buffer is large enough")
            .len();
        out.truncate(len);

        base64::engine::general_purpose::STANDARD.encode(out)
    }

    #[test]
    fn decrypt_reverses_dysons_encryption() {
        let encrypted = encrypt_credential(DEVICE1_SERIAL, DEVICE1_CREDENTIAL);
        let decrypted = decrypt_credential(&encrypted).expect("should decrypt");
        assert_eq!(decrypted, DEVICE1_CREDENTIAL);
    }

    #[test]
    fn decrypt_rejects_garbage() {
        assert!(decrypt_credential("not-base64!").is_err());
        let not_json = base64::engine::general_purpose::STANDARD.encode(b"\x01\x02\x03");
        assert!(decrypt_credential(&not_json).is_err());
    }
}
