// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub static ALL: &[(&str, &str)] = &[
    ("make", include_str!("make.toml")),
    ("du-df", include_str!("du-df.toml")),
    ("ping", include_str!("ping.toml")),
    ("ssh", include_str!("ssh.toml")),
    ("jq", include_str!("jq.toml")),
    ("ps", include_str!("ps.toml")),
    ("mvn-build", include_str!("mvn-build.toml")),
    ("gradle", include_str!("gradle.toml")),
    ("terraform-plan", include_str!("terraform-plan.toml")),
    ("docker", include_str!("docker.toml")),
];
