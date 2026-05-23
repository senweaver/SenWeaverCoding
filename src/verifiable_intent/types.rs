// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Jwk {
    pub kty: String,
    pub crv: String,

    pub x: String,

    pub y: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub d: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cnf {
    pub jwk: Jwk,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kid: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MandateMode {

    Immediate,

    Autonomous,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaymentInstrument {
    #[serde(rename = "type")]
    pub instrument_type: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entity {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    pub website: String,
}

impl Entity {

    pub fn matches(&self, other: &Entity) -> bool {
        match (&self.id, &other.id) {
            (Some(a), Some(b)) => a == b,
            _ => self.name == other.name && self.website == other.website,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcceptableItem {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LineItemEntry {
    pub id: String,
    pub acceptable_items: Vec<AcceptableItem>,
    pub quantity: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FulfillmentLineItem {
    pub item_id: String,
    pub quantity: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum Constraint {

    #[serde(rename = "mandate.checkout.allowed_merchant")]
    AllowedMerchant { allowed_merchants: Vec<Entity> },

    #[serde(rename = "mandate.checkout.line_items")]
    LineItems { items: Vec<LineItemEntry> },

    #[serde(rename = "payment.allowed_payee")]
    AllowedPayee { allowed_payees: Vec<Entity> },

    #[serde(rename = "payment.amount")]
    PaymentAmount {
        currency: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        min: Option<i64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        max: Option<i64>,
    },

    #[serde(rename = "payment.budget")]
    PaymentBudget { currency: String, max: i64 },

    #[serde(rename = "payment.recurrence")]
    PaymentRecurrence {
        frequency: String,
        start_date: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        end_date: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        number: Option<u32>,
    },

    #[serde(rename = "payment.agent_recurrence")]
    AgentRecurrence {
        frequency: String,
        start_date: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        end_date: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        max_occurrences: Option<u32>,
    },

    #[serde(rename = "payment.reference")]
    PaymentReference { conditional_transaction_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalCheckoutMandate {
    pub vct: String,
    pub checkout_jwt: String,
    pub checkout_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalPaymentMandate {
    pub vct: String,
    pub payment_instrument: PaymentInstrument,
    pub currency: String,
    pub amount: i64,
    pub payee: Entity,
    pub transaction_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCheckoutMandate {
    pub vct: String,
    pub cnf: Cnf,
    pub constraints: Vec<Constraint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenPaymentMandate {
    pub vct: String,
    pub cnf: Cnf,
    pub payment_instrument: PaymentInstrument,
    pub constraints: Vec<Constraint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentL3Mandate {
    pub vct: String,
    pub payment_instrument: PaymentInstrument,
    pub payment_amount: PaymentAmount,
    pub payee: Entity,
    pub transaction_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckoutL3Mandate {
    pub vct: String,
    pub checkout_jwt: String,
    pub checkout_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_items: Option<Vec<FulfillmentLineItem>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaymentAmount {
    pub currency: String,
    pub amount: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Fulfillment {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_items: Option<Vec<FulfillmentLineItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merchant: Option<Entity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payee: Option<Entity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_instrument: Option<PaymentInstrument>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer1 {
    pub iss: String,
    pub sub: String,
    pub iat: i64,
    pub exp: i64,
    pub vct: String,
    pub cnf: Cnf,
    pub pan_last_four: String,
    pub scheme: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub card_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer2 {
    pub nonce: String,
    pub aud: String,
    pub iat: i64,
    pub exp: i64,
    pub sd_hash: String,
    pub mode: MandateMode,

    pub mandates: Vec<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct CredentialChain {
    pub l1: Layer1,
    pub l2: Layer2,

    pub l3a: Option<PaymentL3Mandate>,

    pub l3b: Option<CheckoutL3Mandate>,
}
