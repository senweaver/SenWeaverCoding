// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {

    pub model: String,

    pub input_tokens: u64,

    pub output_tokens: u64,

    pub total_tokens: u64,

    pub cost_usd: f64,

    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl TokenUsage {
    fn sanitize_price(value: f64) -> f64 {
        if value.is_finite() && value > 0.0 {
            value
        } else {
            0.0
        }
    }

    pub fn new(
        model: impl Into<String>,
        input_tokens: u64,
        output_tokens: u64,
        input_price_per_million: f64,
        output_price_per_million: f64,
    ) -> Self {
        let model = model.into();
        let input_price_per_million = Self::sanitize_price(input_price_per_million);
        let output_price_per_million = Self::sanitize_price(output_price_per_million);
        let total_tokens = input_tokens.saturating_add(output_tokens);

        let input_cost = (input_tokens as f64 / 1_000_000.0) * input_price_per_million;
        let output_cost = (output_tokens as f64 / 1_000_000.0) * output_price_per_million;
        let cost_usd = input_cost + output_cost;

        Self {
            model,
            input_tokens,
            output_tokens,
            total_tokens,
            cost_usd,
            timestamp: chrono::Utc::now(),
        }
    }

    pub fn cost(&self) -> f64 {
        self.cost_usd
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsagePeriod {
    Session,
    Day,
    Month,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostRecord {

    pub id: String,

    pub usage: TokenUsage,

    pub session_id: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_session_id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coding_mode: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

impl CostRecord {

    pub fn new(session_id: impl Into<String>, usage: TokenUsage) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            usage,
            session_id: session_id.into(),
            chat_session_id: None,
            coding_mode: None,
            provider: None,
        }
    }

    pub fn for_chat_session(
        session_id: impl Into<String>,
        chat_session_id: Option<impl Into<String>>,
        usage: TokenUsage,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            usage,
            session_id: session_id.into(),
            chat_session_id: chat_session_id.map(Into::into),
            coding_mode: None,
            provider: None,
        }
    }

    pub fn for_chat_session_with_mode(
        session_id: impl Into<String>,
        chat_session_id: Option<impl Into<String>>,
        coding_mode: Option<impl Into<String>>,
        usage: TokenUsage,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            usage,
            session_id: session_id.into(),
            chat_session_id: chat_session_id.map(Into::into),
            coding_mode: coding_mode.map(Into::into),
            provider: None,
        }
    }

    pub fn with_attribution(
        session_id: impl Into<String>,
        chat_session_id: Option<impl Into<String>>,
        coding_mode: Option<impl Into<String>>,
        provider: Option<impl Into<String>>,
        usage: TokenUsage,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            usage,
            session_id: session_id.into(),
            chat_session_id: chat_session_id.map(Into::into),
            coding_mode: coding_mode.map(Into::into),
            provider: provider.map(Into::into),
        }
    }
}

#[derive(Debug, Clone)]
pub enum BudgetCheck {

    Allowed,

    Warning {
        current_usd: f64,
        limit_usd: f64,
        period: UsagePeriod,
    },

    Exceeded {
        current_usd: f64,
        limit_usd: f64,
        period: UsagePeriod,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostSummary {

    pub session_cost_usd: f64,

    pub daily_cost_usd: f64,

    pub monthly_cost_usd: f64,

    pub total_tokens: u64,

    pub request_count: usize,

    pub by_model: std::collections::HashMap<String, ModelStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelStats {

    pub model: String,

    pub cost_usd: f64,

    pub total_tokens: u64,

    pub request_count: usize,
}

impl Default for CostSummary {
    fn default() -> Self {
        Self {
            session_cost_usd: 0.0,
            daily_cost_usd: 0.0,
            monthly_cost_usd: 0.0,
            total_tokens: 0,
            request_count: 0,
            by_model: std::collections::HashMap::new(),
        }
    }
}
