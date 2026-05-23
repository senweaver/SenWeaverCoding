// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub trait ChannelConfig {

    fn name() -> &'static str;

    fn desc() -> &'static str;
}

pub trait ConfigHandle {
    fn name(&self) -> &'static str;
    fn desc(&self) -> &'static str;
}
