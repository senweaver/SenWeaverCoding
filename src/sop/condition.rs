// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use serde_json::Value;

pub fn evaluate_condition(condition: &str, payload: Option<&str>) -> bool {
    let condition = condition.trim();
    if condition.is_empty() {
        return true;
    }

    let payload = match payload {
        Some(p) if !p.is_empty() => p,
        _ => return false,
    };

    if let Some(rest) = condition.strip_prefix('$') {

        evaluate_json_path_condition(rest, payload)
    } else {

        evaluate_direct_condition(condition, payload)
    }
}

fn evaluate_json_path_condition(path_and_op: &str, payload: &str) -> bool {
    let json: Value = match serde_json::from_str(payload) {
        Ok(v) => v,
        Err(_) => return false,
    };

    let (dot_path, op, comparand) = match parse_path_op_value(path_and_op) {
        Some(t) => t,
        None => return false,
    };

    let extracted = resolve_json_path(&json, &dot_path);
    let extracted = match extracted {
        Some(v) => v,
        None => return false,
    };

    compare_values(extracted, op, &comparand)
}

fn evaluate_direct_condition(condition: &str, payload: &str) -> bool {
    let (op, comparand) = match parse_op_value(condition) {
        Some(t) => t,
        None => return false,
    };

    let payload_num: f64 = match payload.trim().parse() {
        Ok(n) => n,
        Err(_) => return false,
    };

    let comparand_num: f64 = match comparand.parse() {
        Ok(n) => n,
        Err(_) => return false,
    };

    apply_op_f64(payload_num, op, comparand_num)
}

const OPERATORS: &[&str] = &[">=", "<=", "!=", "==", ">", "<"];

fn parse_path_op_value(input: &str) -> Option<(Vec<&str>, Op, String)> {

    for &op_str in OPERATORS {
        if let Some(pos) = input.find(op_str) {
            let path_part = input[..pos].trim();
            let value_part = input[pos + op_str.len()..].trim();

            if value_part.is_empty() {
                return None;
            }

            let op = Op::from_str(op_str)?;
            let segments: Vec<&str> = path_part.split('.').filter(|s| !s.is_empty()).collect();

            if segments.is_empty() {
                return None;
            }

            return Some((segments, op, value_part.to_string()));
        }
    }
    None
}

fn parse_op_value(input: &str) -> Option<(Op, String)> {
    let input = input.trim();
    for &op_str in OPERATORS {
        if let Some(rest) = input.strip_prefix(op_str) {
            let value = rest.trim();
            if value.is_empty() {
                return None;
            }
            let op = Op::from_str(op_str)?;
            return Some((op, value.to_string()));
        }
    }
    None
}

fn resolve_json_path<'a>(value: &'a Value, segments: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for &seg in segments {

        if let Some(next) = current.get(seg) {
            current = next;
            continue;
        }

        if let Ok(idx) = seg.parse::<usize>() {
            if let Some(next) = current.get(idx) {
                current = next;
                continue;
            }
        }
        return None;
    }
    Some(current)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Gt,
    Lt,
    Gte,
    Lte,
    Eq,
    Neq,
}

impl Op {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            ">" => Some(Self::Gt),
            "<" => Some(Self::Lt),
            ">=" => Some(Self::Gte),
            "<=" => Some(Self::Lte),
            "==" => Some(Self::Eq),
            "!=" => Some(Self::Neq),
            _ => None,
        }
    }
}

fn compare_values(extracted: &Value, op: Op, comparand: &str) -> bool {

    if let Some(lhs) = value_as_f64(extracted) {
        if let Ok(rhs) = comparand.parse::<f64>() {
            return apply_op_f64(lhs, op, rhs);
        }
    }

    let lhs = value_as_string(extracted);

    let rhs = comparand
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(comparand);

    match op {
        Op::Eq => lhs == rhs,
        Op::Neq => lhs != rhs,
        Op::Gt => lhs.as_str() > rhs,
        Op::Lt => lhs.as_str() < rhs,
        Op::Gte => lhs.as_str() >= rhs,
        Op::Lte => lhs.as_str() <= rhs,
    }
}

fn value_as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn value_as_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn apply_op_f64(lhs: f64, op: Op, rhs: f64) -> bool {
    match op {
        Op::Gt => lhs > rhs,
        Op::Lt => lhs < rhs,
        Op::Gte => lhs >= rhs,
        Op::Lte => lhs <= rhs,
        Op::Eq => (lhs - rhs).abs() < f64::EPSILON,
        Op::Neq => (lhs - rhs).abs() >= f64::EPSILON,
    }
}
