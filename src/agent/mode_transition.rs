// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::agent::coding_mode::CodingMode;

pub fn is_auto_approved(
    whitelist: &[String],
    from: CodingMode,
    to: CodingMode,
) -> bool {
    if from == to {
        return true;
    }
    if whitelist.is_empty() {
        return true;
    }
    let from_name = from.display_name();
    let to_name = to.display_name();
    let pair_arrow = format!("{}->{}", from_name, to_name);
    let pair_dash = format!("{}-{}", from_name, to_name);
    let any_to_target = format!("*->{}", to_name);
    let any_from_source = format!("{}->*", from_name);
    whitelist.iter().any(|entry| {
        let token = entry.trim();
        if token.is_empty() {
            return false;
        }
        if token == "*" || token.eq_ignore_ascii_case("all") {
            return true;
        }
        if token.eq_ignore_ascii_case(to_name) {
            return true;
        }
        if token.eq_ignore_ascii_case(&pair_arrow)
            || token.eq_ignore_ascii_case(&pair_dash)
            || token.eq_ignore_ascii_case(&any_to_target)
            || token.eq_ignore_ascii_case(&any_from_source)
        {
            return true;
        }
        false
    })
}
