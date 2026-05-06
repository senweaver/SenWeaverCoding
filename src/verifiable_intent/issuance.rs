// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! L2 and L3 credential issuance.
//!
//! Provides builders for constructing VI credentials with proper SD-JWT
//! serialization and key binding. L1 issuance is out of scope (performed by
//! external credential providers / issuers).

use ring::signature::EcdsaKeyPair;
use serde_json::json;

use crate::verifiable_intent::crypto::{create_disclosure, jws_sign, sd_hash, serialize_sd_jwt};
use crate::verifiable_intent::error::{ViError, ViErrorKind};
use crate::verifiable_intent::types::{
    CheckoutL3Mandate, FinalCheckoutMandate, FinalPaymentMandate, Jwk, OpenCheckoutMandate,
    OpenPaymentMandate, PaymentL3Mandate,
};

#[derive(Debug)]
pub struct ImmediateL2Result {

    pub serialized: String,

    pub sd_hash: String,
}

pub fn create_layer2_immediate(
    serialized_l1: &str,
    checkout: &FinalCheckoutMandate,
    payment: &FinalPaymentMandate,
    audience: &str,
    nonce: &str,
    user_key: &EcdsaKeyPair,
    iat: i64,
    exp: i64,
) -> Result<ImmediateL2Result, ViError> {
    let l1_hash = sd_hash(serialized_l1);

    let checkout_value = serde_json::to_value(checkout).map_err(|e| {
        ViError::new(
            ViErrorKind::IssuanceInputInvalid,
            format!("checkout serialize: {e}"),
        )
    })?;
    let payment_value = serde_json::to_value(payment).map_err(|e| {
        ViError::new(
            ViErrorKind::IssuanceInputInvalid,
            format!("payment serialize: {e}"),
        )
    })?;

    let (checkout_disc, checkout_hash) = create_disclosure("checkout_mandate", &checkout_value)?;
    let (payment_disc, payment_hash) = create_disclosure("payment_mandate", &payment_value)?;

    let header = json!({
        "alg": "ES256",
        "typ": "kb-sd-jwt"
    });

    let payload = json!({
        "nonce": nonce,
        "aud": audience,
        "iat": iat,
        "exp": exp,
        "sd_hash": l1_hash,
        "_sd_alg": "sha-256",
        "_sd": [checkout_hash, payment_hash],
        "delegate_payload": [
            {"...": checkout_hash},
            {"...": payment_hash}
        ]
    });

    let kb_jwt = jws_sign(
        header.to_string().as_bytes(),
        payload.to_string().as_bytes(),
        user_key,
    )?;

    let serialized = serialize_sd_jwt(serialized_l1, &[checkout_disc, payment_disc], Some(&kb_jwt));

    Ok(ImmediateL2Result {
        serialized,
        sd_hash: l1_hash,
    })
}

#[derive(Debug)]
pub struct AutonomousL2Result {

    pub serialized: String,

    pub sd_hash: String,

    pub checkout_disclosure_hash: String,
}

pub fn create_layer2_autonomous(
    serialized_l1: &str,
    checkout: &OpenCheckoutMandate,
    payment: &OpenPaymentMandate,
    audience: &str,
    nonce: &str,
    user_key: &EcdsaKeyPair,
    iat: i64,
    exp: i64,
) -> Result<AutonomousL2Result, ViError> {

    if checkout.cnf != payment.cnf {
        return Err(ViError::new(
            ViErrorKind::ModeMismatch,
            "checkout and payment mandates must bind the same agent key (cnf mismatch)",
        ));
    }

    let l1_hash = sd_hash(serialized_l1);

    let checkout_value = serde_json::to_value(checkout).map_err(|e| {
        ViError::new(
            ViErrorKind::IssuanceInputInvalid,
            format!("checkout serialize: {e}"),
        )
    })?;
    let payment_value = serde_json::to_value(payment).map_err(|e| {
        ViError::new(
            ViErrorKind::IssuanceInputInvalid,
            format!("payment serialize: {e}"),
        )
    })?;

    let (checkout_disc, checkout_hash) = create_disclosure("checkout_mandate", &checkout_value)?;
    let (payment_disc, payment_hash) = create_disclosure("payment_mandate", &payment_value)?;

    let header = json!({
        "alg": "ES256",
        "typ": "kb-sd-jwt+kb"
    });

    let payload = json!({
        "nonce": nonce,
        "aud": audience,
        "iat": iat,
        "exp": exp,
        "sd_hash": l1_hash,
        "_sd_alg": "sha-256",
        "_sd": [checkout_hash, payment_hash],
        "delegate_payload": [
            {"...": checkout_hash},
            {"...": payment_hash}
        ]
    });

    let kb_jwt = jws_sign(
        header.to_string().as_bytes(),
        payload.to_string().as_bytes(),
        user_key,
    )?;

    let serialized = serialize_sd_jwt(serialized_l1, &[checkout_disc, payment_disc], Some(&kb_jwt));

    Ok(AutonomousL2Result {
        serialized,
        sd_hash: l1_hash,
        checkout_disclosure_hash: checkout_hash,
    })
}

#[derive(Debug)]
pub struct L3PaymentResult {

    pub serialized: String,
}

pub fn create_layer3_payment(
    serialized_l2: &str,
    mandate: &PaymentL3Mandate,
    agent_key: &EcdsaKeyPair,
    agent_jwk: &Jwk,
    iat: i64,
    exp: i64,
) -> Result<L3PaymentResult, ViError> {
    let l2_hash = sd_hash(serialized_l2);

    let header = json!({
        "alg": "ES256",
        "typ": "kb-sd-jwt",
        "jwk": agent_jwk,
        "kid": agent_jwk.x
    });

    let mandate_value = serde_json::to_value(mandate).map_err(|e| {
        ViError::new(
            ViErrorKind::IssuanceInputInvalid,
            format!("L3a mandate serialize: {e}"),
        )
    })?;

    let payload = json!({
        "iat": iat,
        "exp": exp,
        "sd_hash": l2_hash,
        "mandate": mandate_value
    });

    let jwt = jws_sign(
        header.to_string().as_bytes(),
        payload.to_string().as_bytes(),
        agent_key,
    )?;

    let serialized = serialize_sd_jwt(&jwt, &[], None);

    Ok(L3PaymentResult { serialized })
}

#[derive(Debug)]
pub struct L3CheckoutResult {

    pub serialized: String,
}

pub fn create_layer3_checkout(
    serialized_l2: &str,
    mandate: &CheckoutL3Mandate,
    agent_key: &EcdsaKeyPair,
    agent_jwk: &Jwk,
    iat: i64,
    exp: i64,
) -> Result<L3CheckoutResult, ViError> {
    let l2_hash = sd_hash(serialized_l2);

    let header = json!({
        "alg": "ES256",
        "typ": "kb-sd-jwt",
        "jwk": agent_jwk,
        "kid": agent_jwk.x
    });

    let mandate_value = serde_json::to_value(mandate).map_err(|e| {
        ViError::new(
            ViErrorKind::IssuanceInputInvalid,
            format!("L3b mandate serialize: {e}"),
        )
    })?;

    let payload = json!({
        "iat": iat,
        "exp": exp,
        "sd_hash": l2_hash,
        "mandate": mandate_value
    });

    let jwt = jws_sign(
        header.to_string().as_bytes(),
        payload.to_string().as_bytes(),
        agent_key,
    )?;

    let serialized = serialize_sd_jwt(&jwt, &[], None);

    Ok(L3CheckoutResult { serialized })
}
