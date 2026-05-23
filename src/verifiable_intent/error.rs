// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViErrorKind {

    InvalidHeader,

    InvalidPayload,

    InvalidDisclosure,

    Expired,

    NotYetValid,

    SignatureInvalid,

    KeyMismatch,

    KeyUnsupported,

    SdHashMismatch,

    CrossReferenceMismatch,

    ReferenceBindingMismatch,

    AmountOutOfRange,

    BudgetExceeded,

    CurrencyMismatch,

    MerchantNotAllowed,

    PayeeNotAllowed,

    LineItemViolation,

    RecurrenceViolation,

    UnknownConstraintType,

    ModeMismatch,

    UnknownMandateType,

    IncompleteMandatePair,

    IssuanceInputInvalid,
}

#[derive(Debug, Clone)]
pub struct ViError {
    pub kind: ViErrorKind,
    pub message: String,
}

impl ViError {
    pub fn new(kind: ViErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for ViError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VI/{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for ViError {}
