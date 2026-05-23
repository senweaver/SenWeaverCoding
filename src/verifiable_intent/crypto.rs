// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::SystemRandom;
use ring::signature::{self, ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};
use sha2::{Digest, Sha256};

use crate::verifiable_intent::error::{ViError, ViErrorKind};
use crate::verifiable_intent::types::Jwk;

pub fn b64u_encode(data: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(data)
}

pub fn b64u_decode(s: &str) -> Result<Vec<u8>, ViError> {
    URL_SAFE_NO_PAD.decode(s).map_err(|e| {
        ViError::new(
            ViErrorKind::InvalidPayload,
            format!("base64url decode: {e}"),
        )
    })
}

pub fn sd_hash(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    b64u_encode(&digest)
}

pub fn sha256(data: &[u8]) -> Vec<u8> {
    Sha256::digest(data).to_vec()
}

pub fn jws_sign(
    header_json: &[u8],
    payload_json: &[u8],
    key_pair: &EcdsaKeyPair,
) -> Result<String, ViError> {
    let header_b64 = b64u_encode(header_json);
    let payload_b64 = b64u_encode(payload_json);
    let signing_input = format!("{header_b64}.{payload_b64}");

    let rng = SystemRandom::new();
    let sig = key_pair.sign(&rng, signing_input.as_bytes()).map_err(|e| {
        ViError::new(
            ViErrorKind::SignatureInvalid,
            format!("signing failed: {e}"),
        )
    })?;

    let sig_b64 = b64u_encode(sig.as_ref());
    Ok(format!("{signing_input}.{sig_b64}"))
}

pub fn jws_verify(compact: &str, public_key_bytes: &[u8]) -> Result<(), ViError> {
    let parts: Vec<&str> = compact.splitn(3, '.').collect();
    if parts.len() != 3 {
        return Err(ViError::new(
            ViErrorKind::InvalidHeader,
            "JWS must have 3 dot-separated parts",
        ));
    }

    let signing_input = format!("{}.{}", parts[0], parts[1]);
    let sig_bytes = b64u_decode(parts[2])?;

    let peer_public_key =
        signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, public_key_bytes);

    peer_public_key
        .verify(signing_input.as_bytes(), &sig_bytes)
        .map_err(|_| {
            ViError::new(
                ViErrorKind::SignatureInvalid,
                "ES256 signature verification failed",
            )
        })
}

pub fn jws_decode_payload(compact: &str) -> Result<serde_json::Value, ViError> {
    let parts: Vec<&str> = compact.splitn(3, '.').collect();
    if parts.len() < 2 {
        return Err(ViError::new(
            ViErrorKind::InvalidPayload,
            "JWS must have at least 2 dot-separated parts",
        ));
    }
    let bytes = b64u_decode(parts[1])?;
    serde_json::from_slice(&bytes)
        .map_err(|e| ViError::new(ViErrorKind::InvalidPayload, format!("payload JSON: {e}")))
}

pub fn jws_decode_header(compact: &str) -> Result<serde_json::Value, ViError> {
    let part = compact
        .split('.')
        .next()
        .ok_or_else(|| ViError::new(ViErrorKind::InvalidHeader, "empty JWS"))?;
    let bytes = b64u_decode(part)?;
    serde_json::from_slice(&bytes)
        .map_err(|e| ViError::new(ViErrorKind::InvalidHeader, format!("header JSON: {e}")))
}

pub fn generate_ec_p256() -> Result<(Vec<u8>, Jwk), ViError> {
    let rng = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng)
        .map_err(|e| ViError::new(ViErrorKind::KeyUnsupported, format!("keygen: {e}")))?;

    let key_pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng)
        .map_err(|e| ViError::new(ViErrorKind::KeyUnsupported, format!("parse pkcs8: {e}")))?;

    let pub_bytes = key_pair.public_key().as_ref();
    let jwk = ec_public_bytes_to_jwk(pub_bytes)?;

    Ok((pkcs8.as_ref().to_vec(), jwk))
}

pub fn load_key_pair(pkcs8_der: &[u8]) -> Result<EcdsaKeyPair, ViError> {
    let rng = SystemRandom::new();
    EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8_der, &rng)
        .map_err(|e| ViError::new(ViErrorKind::KeyUnsupported, format!("load pkcs8: {e}")))
}

pub fn ec_public_bytes_to_jwk(pub_bytes: &[u8]) -> Result<Jwk, ViError> {
    if pub_bytes.len() != 65 || pub_bytes[0] != 0x04 {
        return Err(ViError::new(
            ViErrorKind::KeyUnsupported,
            "expected 65-byte uncompressed EC point (0x04 || x || y)",
        ));
    }
    Ok(Jwk {
        kty: "EC".into(),
        crv: "P-256".into(),
        x: b64u_encode(&pub_bytes[1..33]),
        y: b64u_encode(&pub_bytes[33..65]),
        d: None,
    })
}

pub fn jwk_to_public_bytes(jwk: &Jwk) -> Result<Vec<u8>, ViError> {
    if jwk.kty != "EC" || jwk.crv != "P-256" {
        return Err(ViError::new(
            ViErrorKind::KeyUnsupported,
            format!("unsupported key type: {}:{}", jwk.kty, jwk.crv),
        ));
    }
    let x = b64u_decode(&jwk.x)?;
    let y = b64u_decode(&jwk.y)?;
    if x.len() != 32 || y.len() != 32 {
        return Err(ViError::new(
            ViErrorKind::KeyUnsupported,
            "x/y coordinates must be 32 bytes each",
        ));
    }
    let mut bytes = Vec::with_capacity(65);
    bytes.push(0x04);
    bytes.extend_from_slice(&x);
    bytes.extend_from_slice(&y);
    Ok(bytes)
}

pub fn create_disclosure(
    claim_name: &str,
    claim_value: &serde_json::Value,
) -> Result<(String, String), ViError> {
    let rng = SystemRandom::new();
    let mut salt_bytes = [0u8; 16];
    ring::rand::SecureRandom::fill(&rng, &mut salt_bytes)
        .map_err(|e| ViError::new(ViErrorKind::IssuanceInputInvalid, format!("rng: {e}")))?;
    let salt = b64u_encode(&salt_bytes);

    let disclosure_json = serde_json::json!([salt, claim_name, claim_value]);
    let disclosure_str = serde_json::to_string(&disclosure_json).map_err(|e| {
        ViError::new(
            ViErrorKind::IssuanceInputInvalid,
            format!("disclosure JSON: {e}"),
        )
    })?;
    let disclosure_b64 = b64u_encode(disclosure_str.as_bytes());
    let hash = sd_hash(&disclosure_b64);
    Ok((disclosure_b64, hash))
}

pub fn serialize_sd_jwt(issuer_jwt: &str, disclosures: &[String], kb_jwt: Option<&str>) -> String {
    let mut result = issuer_jwt.to_string();
    for d in disclosures {
        result.push('~');
        result.push_str(d);
    }
    result.push('~');
    if let Some(kb) = kb_jwt {
        result.push_str(kb);
    }
    result
}

pub fn parse_sd_jwt(serialized: &str) -> Result<(&str, Vec<&str>, Option<&str>), ViError> {
    let parts: Vec<&str> = serialized.split('~').collect();
    if parts.len() < 2 {
        return Err(ViError::new(
            ViErrorKind::InvalidDisclosure,
            "SD-JWT must have at least issuer JWT and trailing ~",
        ));
    }
    let issuer_jwt = parts[0];
    let last = parts.last().copied().unwrap_or("");
    let kb_jwt = if last.is_empty() { None } else { Some(last) };

    let disclosures = parts[1..parts.len() - 1].to_vec();

    Ok((issuer_jwt, disclosures, kb_jwt))
}
