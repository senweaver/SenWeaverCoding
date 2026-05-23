// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::verifiable_intent::error::{ViError, ViErrorKind};
use crate::verifiable_intent::types::{
    CheckoutL3Mandate, Constraint, Entity, Fulfillment, LineItemEntry, MandateMode,
    PaymentL3Mandate,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrictnessMode {

    Strict,

    Permissive,
}

#[derive(Debug, Clone)]
pub struct ChainVerificationResult {
    pub valid: bool,
    pub mode: Option<MandateMode>,
    pub errors: Vec<ViError>,
}

impl ChainVerificationResult {
    pub fn ok(mode: MandateMode) -> Self {
        Self {
            valid: true,
            mode: Some(mode),
            errors: vec![],
        }
    }

    pub fn fail(errors: Vec<ViError>) -> Self {
        Self {
            valid: false,
            mode: None,
            errors,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConstraintCheckResult {
    pub satisfied: bool,
    pub constraint_type: String,
    pub violations: Vec<ViError>,
}

impl ConstraintCheckResult {
    pub fn ok(constraint_type: &str) -> Self {
        Self {
            satisfied: true,
            constraint_type: constraint_type.into(),
            violations: vec![],
        }
    }

    pub fn violation(constraint_type: &str, err: ViError) -> Self {
        Self {
            satisfied: false,
            constraint_type: constraint_type.into(),
            violations: vec![err],
        }
    }
}

const CLOCK_SKEW_SECS: i64 = 300;

fn current_timestamp() -> i64 {
    chrono::Utc::now().timestamp()
}

pub fn verify_timestamps(iat: i64, exp: i64) -> Result<(), ViError> {
    let now = current_timestamp();
    if exp + CLOCK_SKEW_SECS < now {
        return Err(ViError::new(
            ViErrorKind::Expired,
            format!("credential expired at {exp}, now {now}"),
        ));
    }
    if iat - CLOCK_SKEW_SECS > now {
        return Err(ViError::new(
            ViErrorKind::NotYetValid,
            format!("credential not valid until {iat}, now {now}"),
        ));
    }
    Ok(())
}

pub fn verify_sd_hash_binding(expected_hash: &str, serialized_parent: &str) -> Result<(), ViError> {
    let computed = crate::verifiable_intent::crypto::sd_hash(serialized_parent);
    if computed != expected_hash {
        return Err(ViError::new(
            ViErrorKind::SdHashMismatch,
            format!("sd_hash mismatch: expected {expected_hash}, computed {computed}"),
        ));
    }
    Ok(())
}

pub fn verify_l3_cross_reference(
    l3a: &PaymentL3Mandate,
    l3b: &CheckoutL3Mandate,
) -> Result<(), ViError> {
    if l3a.transaction_id != l3b.checkout_hash {
        return Err(ViError::new(
            ViErrorKind::CrossReferenceMismatch,
            format!(
                "L3a transaction_id ({}) != L3b checkout_hash ({})",
                l3a.transaction_id, l3b.checkout_hash
            ),
        ));
    }
    Ok(())
}

pub fn verify_checkout_hash_binding(
    checkout_hash: &str,
    checkout_jwt: &str,
) -> Result<(), ViError> {
    let computed = crate::verifiable_intent::crypto::sd_hash(checkout_jwt);
    if computed != checkout_hash {
        return Err(ViError::new(
            ViErrorKind::CrossReferenceMismatch,
            format!("checkout_hash mismatch: expected {checkout_hash}, computed {computed}"),
        ));
    }
    Ok(())
}

pub fn infer_mode_from_vct(vct: &str) -> Result<MandateMode, ViError> {
    match vct {
        "mandate.checkout" | "mandate.payment" => Ok(MandateMode::Immediate),
        "mandate.checkout.open" | "mandate.payment.open" => Ok(MandateMode::Autonomous),
        _ => Err(ViError::new(
            ViErrorKind::UnknownMandateType,
            format!("unrecognized mandate VCT: {vct}"),
        )),
    }
}

pub fn check_constraints(
    constraints: &[Constraint],
    fulfillment: &Fulfillment,
    strictness: StrictnessMode,
) -> Vec<ConstraintCheckResult> {
    constraints
        .iter()
        .map(|c| check_single_constraint(c, fulfillment, strictness))
        .collect()
}

fn check_single_constraint(
    constraint: &Constraint,
    fulfillment: &Fulfillment,
    _strictness: StrictnessMode,
) -> ConstraintCheckResult {
    match constraint {
        Constraint::AllowedMerchant { allowed_merchants } => {
            check_allowed_merchant(allowed_merchants, fulfillment)
        }
        Constraint::LineItems { items } => check_line_items(items, fulfillment),
        Constraint::AllowedPayee { allowed_payees } => {
            check_allowed_payee(allowed_payees, fulfillment)
        }
        Constraint::PaymentAmount { currency, min, max } => {
            check_payment_amount(currency, *min, *max, fulfillment)
        }
        Constraint::PaymentBudget { currency, max } => {
            check_payment_budget(currency, *max, fulfillment)
        }
        Constraint::PaymentReference {
            conditional_transaction_id,
        } => {

            ConstraintCheckResult::ok(&format!(
                "payment.reference({})",
                &conditional_transaction_id[..8.min(conditional_transaction_id.len())]
            ))
        }
        Constraint::PaymentRecurrence { .. } | Constraint::AgentRecurrence { .. } => {

            ConstraintCheckResult::ok("recurrence")
        }
    }
}

fn check_allowed_merchant(
    allowed_merchants: &[Entity],
    fulfillment: &Fulfillment,
) -> ConstraintCheckResult {
    let ct = "mandate.checkout.allowed_merchant";
    if allowed_merchants.is_empty() {
        return ConstraintCheckResult::violation(
            ct,
            ViError::new(
                ViErrorKind::MerchantNotAllowed,
                "empty merchant allowlist is unsatisfiable",
            ),
        );
    }
    let Some(merchant) = &fulfillment.merchant else {

        return ConstraintCheckResult::ok(ct);
    };
    if allowed_merchants.iter().any(|m| m.matches(merchant)) {
        ConstraintCheckResult::ok(ct)
    } else {
        ConstraintCheckResult::violation(
            ct,
            ViError::new(
                ViErrorKind::MerchantNotAllowed,
                format!("merchant '{}' not in allowed list", merchant.name),
            ),
        )
    }
}

fn check_allowed_payee(
    allowed_payees: &[Entity],
    fulfillment: &Fulfillment,
) -> ConstraintCheckResult {
    let ct = "payment.allowed_payee";
    if allowed_payees.is_empty() {
        return ConstraintCheckResult::violation(
            ct,
            ViError::new(
                ViErrorKind::PayeeNotAllowed,
                "empty payee allowlist is unsatisfiable",
            ),
        );
    }
    let Some(payee) = &fulfillment.payee else {
        return ConstraintCheckResult::ok(ct);
    };
    if allowed_payees.iter().any(|p| p.matches(payee)) {
        ConstraintCheckResult::ok(ct)
    } else {
        ConstraintCheckResult::violation(
            ct,
            ViError::new(
                ViErrorKind::PayeeNotAllowed,
                format!("payee '{}' not in allowed list", payee.name),
            ),
        )
    }
}

fn check_payment_amount(
    currency: &str,
    min: Option<i64>,
    max: Option<i64>,
    fulfillment: &Fulfillment,
) -> ConstraintCheckResult {
    let ct = "payment.amount";
    let Some(actual_amount) = fulfillment.amount else {
        return ConstraintCheckResult::violation(
            ct,
            ViError::new(
                ViErrorKind::AmountOutOfRange,
                "missing payment amount in fulfillment",
            ),
        );
    };
    if let Some(actual_currency) = &fulfillment.currency {
        if actual_currency != currency {
            return ConstraintCheckResult::violation(
                ct,
                ViError::new(
                    ViErrorKind::CurrencyMismatch,
                    format!("expected {currency}, got {actual_currency}"),
                ),
            );
        }
    }
    if let Some(max_val) = max {
        if actual_amount > max_val {
            return ConstraintCheckResult::violation(
                ct,
                ViError::new(
                    ViErrorKind::AmountOutOfRange,
                    format!("amount {actual_amount} > max {max_val} {currency}"),
                ),
            );
        }
    }
    if let Some(min_val) = min {
        if actual_amount < min_val {
            return ConstraintCheckResult::violation(
                ct,
                ViError::new(
                    ViErrorKind::AmountOutOfRange,
                    format!("amount {actual_amount} < min {min_val} {currency}"),
                ),
            );
        }
    }
    ConstraintCheckResult::ok(ct)
}

fn check_payment_budget(
    currency: &str,
    max: i64,
    fulfillment: &Fulfillment,
) -> ConstraintCheckResult {
    let ct = "payment.budget";
    let Some(actual_amount) = fulfillment.amount else {
        return ConstraintCheckResult::violation(
            ct,
            ViError::new(
                ViErrorKind::BudgetExceeded,
                "missing payment amount in fulfillment",
            ),
        );
    };
    if let Some(actual_currency) = &fulfillment.currency {
        if actual_currency != currency {
            return ConstraintCheckResult::violation(
                ct,
                ViError::new(
                    ViErrorKind::CurrencyMismatch,
                    format!("expected {currency}, got {actual_currency}"),
                ),
            );
        }
    }

    if actual_amount > max {
        return ConstraintCheckResult::violation(
            ct,
            ViError::new(
                ViErrorKind::BudgetExceeded,
                format!("amount {actual_amount} > budget max {max} {currency}"),
            ),
        );
    }
    ConstraintCheckResult::ok(ct)
}

fn check_line_items(
    constraint_items: &[LineItemEntry],
    fulfillment: &Fulfillment,
) -> ConstraintCheckResult {
    let ct = "mandate.checkout.line_items";
    if constraint_items.is_empty() {
        return ConstraintCheckResult::violation(
            ct,
            ViError::new(
                ViErrorKind::LineItemViolation,
                "empty items allowlist is unsatisfiable",
            ),
        );
    }
    let Some(fulfillment_items) = &fulfillment.line_items else {
        return ConstraintCheckResult::violation(
            ct,
            ViError::new(
                ViErrorKind::LineItemViolation,
                "empty cart does not satisfy line_items constraint",
            ),
        );
    };
    if fulfillment_items.is_empty() {
        return ConstraintCheckResult::violation(
            ct,
            ViError::new(
                ViErrorKind::LineItemViolation,
                "empty cart does not satisfy line_items constraint",
            ),
        );
    }

    let total_allowed: u32 = constraint_items.iter().map(|l| l.quantity).sum();
    let total_actual: u32 = fulfillment_items.iter().map(|f| f.quantity).sum();
    if total_actual > total_allowed {
        return ConstraintCheckResult::violation(
            ct,
            ViError::new(
                ViErrorKind::LineItemViolation,
                format!("total quantity {total_actual} > allowed {total_allowed}"),
            ),
        );
    }

    for fi in fulfillment_items {
        let allowed_by_any = constraint_items.iter().any(|entry| {
            if entry.acceptable_items.is_empty() {
                return true;
            }
            entry.acceptable_items.iter().any(|ai| ai.id == fi.item_id)
        });
        if !allowed_by_any {
            return ConstraintCheckResult::violation(
                ct,
                ViError::new(
                    ViErrorKind::LineItemViolation,
                    format!("item '{}' not in any acceptable_items list", fi.item_id),
                ),
            );
        }
    }

    ConstraintCheckResult::ok(ct)
}
