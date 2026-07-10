// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::atomic::{AtomicU8, Ordering};

const OWNER_NONE: u8 = 0;
const OWNER_RECORDING: u8 = 1;
const OWNER_REPLAY: u8 = 2;
const OWNER_AGENT: u8 = 3;

static OWNER: AtomicU8 = AtomicU8::new(OWNER_NONE);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputActivity {
    Recording,
    Replay,
    Agent,
}

impl InputActivity {
    fn code(self) -> u8 {
        match self {
            InputActivity::Recording => OWNER_RECORDING,
            InputActivity::Replay => OWNER_REPLAY,
            InputActivity::Agent => OWNER_AGENT,
        }
    }

    fn label(code: u8) -> &'static str {
        match code {
            OWNER_RECORDING => "a recording",
            OWNER_REPLAY => "a replay",
            OWNER_AGENT => "an automation run",
            _ => "another activity",
        }
    }
}

#[must_use = "dropping the lease releases the input lock"]
pub struct InputLease {
    _private: (),
}

impl Drop for InputLease {
    fn drop(&mut self) {
        OWNER.store(OWNER_NONE, Ordering::Release);
    }
}

pub fn try_acquire(activity: InputActivity) -> Result<InputLease, String> {
    match OWNER.compare_exchange(
        OWNER_NONE,
        activity.code(),
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => Ok(InputLease { _private: () }),
        Err(current) => Err(format!(
            "computer control is busy with {}; stop it before starting a new action",
            InputActivity::label(current)
        )),
    }
}
