// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use hmac::{Hmac, Mac};
use sha2::Sha256;

pub fn verify_hmac_sha256_hex(secret: &[u8], body: &[u8], expected_hex: &str) -> bool {
    let Ok(mut mac) = <Hmac<Sha256>>::new_from_slice(secret) else {
        return false;
    };
    mac.update(body);
    let digest = mac.finalize().into_bytes();
    let actual_hex = hex::encode(digest);
    constant_time_eq_ignore_case(&actual_hex, expected_hex)
}

fn constant_time_eq_ignore_case(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.bytes().zip(b.bytes()) {
        diff |= x.to_ascii_lowercase() ^ y.to_ascii_lowercase();
    }
    diff == 0
}
