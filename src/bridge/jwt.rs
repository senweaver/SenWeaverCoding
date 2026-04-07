// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// JWT utilities for bridge authentication — mirrors claude-code-typescript-src`bridge/jwtUtils.ts`.

use serde::{Deserialize, Serialize};

/// Claims embedded in a bridge JWT.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeClaims {
    /// Subject (device ID).
    pub sub: String,
    /// Session ID.
    pub session_id: String,
    /// Issued at (epoch seconds).
    pub iat: u64,
    /// Expiration (epoch seconds).
    pub exp: u64,
    /// Issuer.
    pub iss: String,
}

/// Generate a simple HMAC-SHA256 bridge token.
/// In production, use a proper JWT library (jsonwebtoken crate).
pub fn generate_bridge_token(
    device_id: &str,
    session_id: &str,
    secret: &[u8],
    ttl_secs: u64,
) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let claims = BridgeClaims {
        sub: device_id.to_string(),
        session_id: session_id.to_string(),
        iat: now,
        exp: now + ttl_secs,
        iss: "sen-bridge".to_string(),
    };

    let payload = serde_json::to_string(&claims).unwrap_or_default();
    let header = r#"{"alg":"HS256","typ":"JWT"}"#;

    let encoded_header = base64_url_encode(header.as_bytes());
    let encoded_payload = base64_url_encode(payload.as_bytes());
    let signing_input = format!("{encoded_header}.{encoded_payload}");

    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC key length");
    mac.update(signing_input.as_bytes());
    let signature = mac.finalize().into_bytes();
    let encoded_sig = base64_url_encode(&signature);

    format!("{signing_input}.{encoded_sig}")
}

/// Verify a bridge token and return the claims.
pub fn verify_bridge_token(token: &str, secret: &[u8]) -> Result<BridgeClaims, BridgeTokenError> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(BridgeTokenError::MalformedToken);
    }

    let signing_input = format!("{}.{}", parts[0], parts[1]);

    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(secret).map_err(|_| BridgeTokenError::InvalidKey)?;
    mac.update(signing_input.as_bytes());

    let expected_sig = base64_url_encode(&mac.finalize().into_bytes());
    if expected_sig != parts[2] {
        return Err(BridgeTokenError::InvalidSignature);
    }

    let payload_bytes =
        base64_url_decode(parts[1]).map_err(|_| BridgeTokenError::MalformedToken)?;
    let claims: BridgeClaims =
        serde_json::from_slice(&payload_bytes).map_err(|_| BridgeTokenError::MalformedToken)?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if now > claims.exp {
        return Err(BridgeTokenError::Expired);
    }

    Ok(claims)
}

#[derive(Debug, thiserror::Error)]
pub enum BridgeTokenError {
    #[error("malformed token")]
    MalformedToken,
    #[error("invalid signature")]
    InvalidSignature,
    #[error("invalid key")]
    InvalidKey,
    #[error("token expired")]
    Expired,
}

fn base64_url_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

fn base64_url_decode(s: &str) -> Result<Vec<u8>, base64::DecodeError> {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s)
}
