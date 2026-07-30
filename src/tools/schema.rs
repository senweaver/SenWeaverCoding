// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};

pub const GEMINI_UNSUPPORTED_KEYWORDS: &[&str] = &[

    "$ref",
    "$schema",
    "$id",
    "$defs",
    "definitions",

    "additionalProperties",
    "patternProperties",

    "multipleOf",

    "uniqueItems",

    "minProperties",
    "maxProperties",

    "examples",
];

pub const OPENAI_STRICT_UNSUPPORTED_KEYWORDS: &[&str] = &[

    "$schema",
    "$id",
    "$defs",
    "definitions",

    "minLength",
    "maxLength",
    "pattern",
    "format",

    "minimum",
    "maximum",
    "multipleOf",

    "minItems",
    "maxItems",
    "uniqueItems",

    "patternProperties",
    "propertyNames",
    "minProperties",
    "maxProperties",
    "unevaluatedProperties",
];

const SCHEMA_META_KEYS: &[&str] = &["description", "title", "default"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleaningStrategy {

    Gemini,

    Anthropic,

    OpenAI,

    Conservative,
}

impl CleaningStrategy {

    pub fn unsupported_keywords(self) -> &'static [&'static str] {
        match self {
            Self::Gemini => GEMINI_UNSUPPORTED_KEYWORDS,
            Self::Anthropic => &["$ref", "$defs", "definitions"],
            Self::OpenAI => OPENAI_STRICT_UNSUPPORTED_KEYWORDS,
            Self::Conservative => &["$ref", "$defs", "definitions", "additionalProperties"],
        }
    }
}

pub struct SchemaCleanr;

impl SchemaCleanr {

    pub fn clean_for_gemini(schema: Value) -> Value {
        Self::clean(schema, CleaningStrategy::Gemini)
    }

    pub fn clean_for_anthropic(schema: Value) -> Value {
        Self::clean(schema, CleaningStrategy::Anthropic)
    }

    pub fn clean_for_openai(schema: Value) -> Value {
        Self::clean(schema, CleaningStrategy::OpenAI)
    }

    pub fn prepare_for_strict_output(schema: Value) -> Value {
        let cleaned = Self::clean(schema, CleaningStrategy::OpenAI);
        Self::enforce_strict_node(cleaned)
    }

    fn enforce_strict_node(value: Value) -> Value {
        let Value::Object(mut obj) = value else {
            return value;
        };

        obj.remove("$defs");
        obj.remove("definitions");
        obj.remove("$schema");
        obj.remove("$id");

        let is_object_schema = obj.get("type").and_then(Value::as_str) == Some("object")
            || obj
                .get("type")
                .and_then(Value::as_array)
                .is_some_and(|types| types.iter().any(|t| t.as_str() == Some("object")))
            || obj.contains_key("properties");

        for key in ["anyOf", "oneOf", "allOf"] {
            if let Some(Value::Array(variants)) = obj.remove(key) {
                let enforced: Vec<Value> = variants
                    .into_iter()
                    .map(Self::enforce_strict_node)
                    .collect();
                obj.insert(key.to_string(), Value::Array(enforced));
            }
        }

        if let Some(items) = obj.remove("items") {
            let enforced = match items {
                Value::Array(entries) => Value::Array(
                    entries.into_iter().map(Self::enforce_strict_node).collect(),
                ),
                other => Self::enforce_strict_node(other),
            };
            obj.insert("items".to_string(), enforced);
        }

        if is_object_schema {
            if !obj.contains_key("type") {
                obj.insert("type".to_string(), Value::String("object".to_string()));
            }
            let existing_required: HashSet<String> = obj
                .get("required")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();

            if let Some(Value::Object(props)) = obj.remove("properties") {
                let mut enforced_props = Map::new();
                let mut required = Vec::with_capacity(props.len());
                for (key, prop) in props {
                    let mut enforced = Self::enforce_strict_node(prop);
                    if !existing_required.contains(&key) {
                        Self::make_nullable(&mut enforced);
                    }
                    required.push(Value::String(key.clone()));
                    enforced_props.insert(key, enforced);
                }
                obj.insert("properties".to_string(), Value::Object(enforced_props));
                obj.insert("required".to_string(), Value::Array(required));
            } else {
                obj.insert("properties".to_string(), Value::Object(Map::new()));
                obj.insert("required".to_string(), Value::Array(Vec::new()));
            }
            obj.insert("additionalProperties".to_string(), Value::Bool(false));
        } else if let Some(additional) = obj.remove("additionalProperties") {
            if matches!(additional, Value::Object(_)) {
                obj.insert(
                    "additionalProperties".to_string(),
                    Self::enforce_strict_node(additional),
                );
            } else {
                obj.insert("additionalProperties".to_string(), additional);
            }
        }

        Value::Object(obj)
    }

    fn make_nullable(prop: &mut Value) {
        let Some(obj) = prop.as_object_mut() else {
            return;
        };

        if let Some(Value::Array(variants)) = obj.get_mut("anyOf") {
            let has_null = variants.iter().any(Self::is_null_schema);
            if !has_null {
                variants.push(json!({ "type": "null" }));
            }
            return;
        }

        match obj.get_mut("type") {
            Some(Value::String(t)) => {
                if t != "null" {
                    let current = t.clone();
                    obj.insert(
                        "type".to_string(),
                        json!([current, "null"]),
                    );
                }
                if let Some(Value::Array(values)) = obj.get_mut("enum") {
                    if !values.iter().any(Value::is_null) {
                        values.push(Value::Null);
                    }
                }
            }
            Some(Value::Array(types)) => {
                if !types.iter().any(|t| t.as_str() == Some("null")) {
                    types.push(Value::String("null".to_string()));
                }
                if let Some(Value::Array(values)) = obj.get_mut("enum") {
                    if !values.iter().any(Value::is_null) {
                        values.push(Value::Null);
                    }
                }
            }
            _ => {
                let inner = Value::Object(obj.clone());
                *prop = json!({ "anyOf": [inner, { "type": "null" }] });
            }
        }
    }

    pub fn clean(schema: Value, strategy: CleaningStrategy) -> Value {

        let defs = if let Some(obj) = schema.as_object() {
            Self::extract_defs(obj)
        } else {
            HashMap::new()
        };

        Self::clean_with_defs(schema, &defs, strategy, &mut HashSet::new())
    }

    pub fn validate(schema: &Value) -> anyhow::Result<()> {
        let obj = schema
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("Schema must be an object"))?;

        if !obj.contains_key("type") {
            anyhow::bail!("Schema missing required 'type' field");
        }

        if let Some(Value::String(t)) = obj.get("type") {
            if t == "object" && !obj.contains_key("properties") {
                tracing::warn!("Object schema without 'properties' field may cause issues");
            }
        }

        Ok(())
    }

    fn extract_defs(obj: &Map<String, Value>) -> HashMap<String, Value> {
        let mut defs = HashMap::new();
        Self::collect_defs_from_object(obj, &mut defs);
        defs
    }

    fn collect_defs_from_object(obj: &Map<String, Value>, defs: &mut HashMap<String, Value>) {
        for container_key in ["$defs", "definitions"] {
            if let Some(Value::Object(defs_obj)) = obj.get(container_key) {
                for (key, value) in defs_obj {
                    defs.entry(key.clone()).or_insert_with(|| value.clone());
                }
            }
        }
        for value in obj.values() {
            Self::collect_defs_from_value(value, defs);
        }
    }

    fn collect_defs_from_value(value: &Value, defs: &mut HashMap<String, Value>) {
        match value {
            Value::Object(obj) => Self::collect_defs_from_object(obj, defs),
            Value::Array(arr) => {
                for entry in arr {
                    Self::collect_defs_from_value(entry, defs);
                }
            }
            _ => {}
        }
    }

    fn clean_with_defs(
        schema: Value,
        defs: &HashMap<String, Value>,
        strategy: CleaningStrategy,
        ref_stack: &mut HashSet<String>,
    ) -> Value {
        match schema {
            Value::Object(obj) => Self::clean_object(obj, defs, strategy, ref_stack),
            Value::Array(arr) => Value::Array(
                arr.into_iter()
                    .map(|v| Self::clean_with_defs(v, defs, strategy, ref_stack))
                    .collect(),
            ),
            other => other,
        }
    }

    fn clean_object(
        obj: Map<String, Value>,
        defs: &HashMap<String, Value>,
        strategy: CleaningStrategy,
        ref_stack: &mut HashSet<String>,
    ) -> Value {

        if let Some(Value::String(ref_value)) = obj.get("$ref") {
            return Self::resolve_ref(ref_value, &obj, defs, strategy, ref_stack);
        }

        let union_key = if obj.contains_key("anyOf") {
            Some("anyOf")
        } else if obj.contains_key("oneOf") {
            Some("oneOf")
        } else {
            None
        };
        let mut precleaned_union: Option<Vec<Value>> = None;
        if let Some(active_key) = union_key {
            if let Some(Value::Array(variants)) = obj.get(active_key) {
                let cleaned_variants =
                    Self::clean_union_variants(variants, defs, strategy, ref_stack);
                if let Some(simplified) = Self::simplify_cleaned_union(&obj, &cleaned_variants) {
                    return simplified;
                }
                precleaned_union = Some(cleaned_variants);
            }
        }

        let mut cleaned = Map::new();
        let unsupported: HashSet<&str> = strategy.unsupported_keywords().iter().copied().collect();
        let has_union = obj.contains_key("anyOf") || obj.contains_key("oneOf");

        for (key, value) in obj {

            if unsupported.contains(key.as_str()) {
                continue;
            }

            match key.as_str() {

                "const" => {
                    cleaned.insert("enum".to_string(), json!([value]));
                }

                "type" if has_union => {

                }

                "type" if matches!(value, Value::Array(_)) => {
                    let (cleaned_value, dropped_null) = Self::clean_type_array(value);
                    if dropped_null && strategy == CleaningStrategy::Gemini {
                        cleaned.insert("nullable".to_string(), Value::Bool(true));
                    }
                    cleaned.insert(key, cleaned_value);
                }

                "properties" => {
                    let cleaned_value = Self::clean_properties(value, defs, strategy, ref_stack);
                    cleaned.insert(key, cleaned_value);
                }
                "items" => {
                    let cleaned_value = Self::clean_with_defs(value, defs, strategy, ref_stack);
                    cleaned.insert(key, cleaned_value);
                }
                "anyOf" | "oneOf" | "allOf" => {
                    let cleaned_value = if union_key == Some(key.as_str()) {
                        match precleaned_union.take() {
                            Some(variants) => Value::Array(variants),
                            None => Self::clean_union(value, defs, strategy, ref_stack),
                        }
                    } else {
                        Self::clean_union(value, defs, strategy, ref_stack)
                    };
                    cleaned.insert(key, cleaned_value);
                }

                _ => {
                    let cleaned_value = match value {
                        Value::Object(_) | Value::Array(_) => {
                            Self::clean_with_defs(value, defs, strategy, ref_stack)
                        }
                        other => other,
                    };
                    cleaned.insert(key, cleaned_value);
                }
            }
        }

        Value::Object(cleaned)
    }

    fn resolve_ref(
        ref_value: &str,
        obj: &Map<String, Value>,
        defs: &HashMap<String, Value>,
        strategy: CleaningStrategy,
        ref_stack: &mut HashSet<String>,
    ) -> Value {

        if ref_stack.contains(ref_value) {
            tracing::warn!("Circular $ref detected: {}", ref_value);
            return Self::preserve_meta(obj, Self::degraded_ref_fallback(strategy));
        }

        if let Some(def_name) = Self::parse_local_ref(ref_value) {
            if let Some(definition) = defs.get(def_name.as_str()) {
                ref_stack.insert(ref_value.to_string());
                let cleaned = Self::clean_with_defs(definition.clone(), defs, strategy, ref_stack);
                ref_stack.remove(ref_value);
                return Self::preserve_meta(obj, cleaned);
            }
        }

        tracing::warn!("Cannot resolve $ref: {}", ref_value);
        Self::preserve_meta(obj, Self::degraded_ref_fallback(strategy))
    }

    fn degraded_ref_fallback(strategy: CleaningStrategy) -> Value {
        match strategy {
            CleaningStrategy::OpenAI => json!({
                "type": "string",
                "description": "JSON-encoded object (schema reference could not be resolved; provide the value as a JSON string)"
            }),
            _ if strategy
                .unsupported_keywords()
                .contains(&"additionalProperties") =>
            {
                json!({ "type": "object" })
            }
            _ => json!({ "type": "object", "additionalProperties": true }),
        }
    }

    fn parse_local_ref(ref_value: &str) -> Option<String> {
        ref_value
            .strip_prefix("#/$defs/")
            .or_else(|| ref_value.strip_prefix("#/definitions/"))
            .map(Self::decode_json_pointer)
    }

    fn decode_json_pointer(segment: &str) -> String {
        if !segment.contains('~') {
            return segment.to_string();
        }

        let mut decoded = String::with_capacity(segment.len());
        let mut chars = segment.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '~' {
                match chars.peek().copied() {
                    Some('0') => {
                        chars.next();
                        decoded.push('~');
                    }
                    Some('1') => {
                        chars.next();
                        decoded.push('/');
                    }
                    _ => decoded.push('~'),
                }
            } else {
                decoded.push(ch);
            }
        }

        decoded
    }

    fn clean_union_variants(
        variants: &[Value],
        defs: &HashMap<String, Value>,
        strategy: CleaningStrategy,
        ref_stack: &mut HashSet<String>,
    ) -> Vec<Value> {
        variants
            .iter()
            .map(|v| Self::clean_with_defs(v.clone(), defs, strategy, ref_stack))
            .collect()
    }

    fn simplify_cleaned_union(
        obj: &Map<String, Value>,
        cleaned_variants: &[Value],
    ) -> Option<Value> {
        let non_null: Vec<Value> = cleaned_variants
            .iter()
            .filter(|v| !Self::is_null_schema(v))
            .cloned()
            .collect();

        if non_null.len() == 1 {
            return Some(Self::preserve_meta(obj, non_null[0].clone()));
        }

        if let Some(enum_value) = Self::try_flatten_literal_union(&non_null) {
            return Some(Self::preserve_meta(obj, enum_value));
        }

        None
    }

    fn is_null_schema(value: &Value) -> bool {
        if let Some(obj) = value.as_object() {

            if let Some(Value::Null) = obj.get("const") {
                return true;
            }

            if let Some(Value::Array(arr)) = obj.get("enum") {
                if arr.len() == 1 && matches!(arr[0], Value::Null) {
                    return true;
                }
            }

            if let Some(Value::String(t)) = obj.get("type") {
                if t == "null" {
                    return true;
                }
            }
        }
        false
    }

    fn try_flatten_literal_union(variants: &[Value]) -> Option<Value> {
        if variants.is_empty() {
            return None;
        }

        let mut all_values = Vec::new();
        let mut common_type: Option<String> = None;

        for variant in variants {
            let obj = variant.as_object()?;

            let literal_value = if let Some(const_val) = obj.get("const") {
                const_val.clone()
            } else if let Some(Value::Array(arr)) = obj.get("enum") {
                if arr.len() == 1 {
                    arr[0].clone()
                } else {
                    return None;
                }
            } else {
                return None;
            };

            let variant_type = obj.get("type")?.as_str()?;
            match &common_type {
                None => common_type = Some(variant_type.to_string()),
                Some(t) if t != variant_type => return None,
                _ => {}
            }

            all_values.push(literal_value);
        }

        common_type.map(|t| {
            json!({
                "type": t,
                "enum": all_values
            })
        })
    }

    fn clean_type_array(value: Value) -> (Value, bool) {
        if let Value::Array(types) = value {
            let original_len = types.len();
            let non_null: Vec<Value> = types
                .into_iter()
                .filter(|v| v.as_str() != Some("null"))
                .collect();
            let dropped_null = non_null.len() < original_len && !non_null.is_empty();

            let cleaned = match non_null.len() {
                0 => Value::String("null".to_string()),
                1 => non_null
                    .into_iter()
                    .next()
                    .unwrap_or(Value::String("null".to_string())),
                _ => Value::Array(non_null),
            };
            (cleaned, dropped_null)
        } else {
            (value, false)
        }
    }

    fn clean_properties(
        value: Value,
        defs: &HashMap<String, Value>,
        strategy: CleaningStrategy,
        ref_stack: &mut HashSet<String>,
    ) -> Value {
        if let Value::Object(props) = value {
            let cleaned: Map<String, Value> = props
                .into_iter()
                .map(|(k, v)| (k, Self::clean_with_defs(v, defs, strategy, ref_stack)))
                .collect();
            Value::Object(cleaned)
        } else {
            value
        }
    }

    fn clean_union(
        value: Value,
        defs: &HashMap<String, Value>,
        strategy: CleaningStrategy,
        ref_stack: &mut HashSet<String>,
    ) -> Value {
        if let Value::Array(variants) = value {
            let cleaned: Vec<Value> = variants
                .into_iter()
                .map(|v| Self::clean_with_defs(v, defs, strategy, ref_stack))
                .collect();
            Value::Array(cleaned)
        } else {
            value
        }
    }

    fn preserve_meta(source: &Map<String, Value>, mut target: Value) -> Value {
        if let Value::Object(target_obj) = &mut target {
            for &key in SCHEMA_META_KEYS {
                if let Some(value) = source.get(key) {
                    target_obj.insert(key.to_string(), value.clone());
                }
            }
        }
        target
    }
}
