// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::config::schema::{Config, ModelPricing};
use std::collections::HashMap;

pub fn lookup_model_pricing<'a>(
    prices: &'a HashMap<String, ModelPricing>,
    provider_name: &str,
    model: &str,
) -> Option<&'a ModelPricing> {
    let model = model.trim();
    if model.is_empty() {
        return None;
    }

    let provider = provider_name.trim();
    if !provider.is_empty() {
        if let Some(hit) = prices.get(&format!("{provider}/{model}")) {
            return Some(hit);
        }
    }

    if let Some(hit) = prices.get(model) {
        return Some(hit);
    }

    if let Some((_, suffix)) = model.rsplit_once('/') {
        if let Some(hit) = prices.get(suffix) {
            return Some(hit);
        }
    }

    let bare = model.rsplit_once('/').map(|(_, s)| s).unwrap_or(model);
    prices.iter().find_map(|(key, value)| {
        let key_bare = key.rsplit_once('/').map(|(_, s)| s).unwrap_or(key.as_str());
        if key.eq_ignore_ascii_case(model) || key_bare.eq_ignore_ascii_case(bare) {
            Some(value)
        } else {
            None
        }
    })
}

pub fn effective_model_prices(config: &Config) -> HashMap<String, ModelPricing> {
    let mut prices = crate::config::schema::model_pricing::get_default_pricing();

    for (key, value) in &config.cost.prices {
        prices.insert(key.clone(), value.clone());
    }

    for (provider_id, profile) in &config.model_providers {
        for (model, pricing) in &profile.model_pricing {
            let model = model.trim();
            if model.is_empty() {
                continue;
            }
            prices.insert(format!("{provider_id}/{model}"), pricing.clone());
            if let Some(name) = profile.name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                prices.insert(format!("{name}/{model}"), pricing.clone());
            }
            prices
                .entry(model.to_string())
                .or_insert_with(|| pricing.clone());
        }
    }

    prices
}
