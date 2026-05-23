// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub const REDACTED: &str = "***REDACTED***";

pub fn redact_optional_string<S: Serializer>(
    value: &Option<String>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match value {
        Some(_) => serializer.serialize_some(REDACTED),
        None => serializer.serialize_none(),
    }
}

pub fn deserialize_optional_string<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)
}

pub fn redact_string<S: Serializer>(_value: &String, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(REDACTED)
}

pub fn deserialize_plain_string<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    String::deserialize(deserializer)
}

pub fn redact_vec_string<S: Serializer>(
    value: &Vec<String>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeSeq;
    let mut seq = serializer.serialize_seq(Some(value.len()))?;
    for _ in value {
        seq.serialize_element(REDACTED)?;
    }
    seq.end()
}

pub fn is_redacted(s: &str) -> bool {
    s == REDACTED
}

pub fn strip_redacted_from_vec(v: Vec<String>) -> Vec<String> {
    v.into_iter().filter(|s| !is_redacted(s)).collect()
}

pub fn serialize_exposed_optional_string<S: Serializer>(
    value: &Option<String>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    value.serialize(serializer)
}
