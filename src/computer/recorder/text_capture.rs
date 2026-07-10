// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

#[cfg(windows)]
const FOCUSED_TEXT_TIMEOUT_MS: u64 = 150;

#[cfg(windows)]
pub async fn focused_text() -> Option<String> {
    let handle = tokio::task::spawn_blocking(imp::focused_text_blocking);
    match tokio::time::timeout(
        std::time::Duration::from_millis(FOCUSED_TEXT_TIMEOUT_MS),
        handle,
    )
    .await
    {
        Ok(join) => join.ok().flatten(),
        Err(_) => None,
    }
}

#[cfg(not(windows))]
pub async fn focused_text() -> Option<String> {
    None
}

pub fn typed_delta(baseline: Option<&str>, current: Option<&str>, fallback: &str) -> String {
    match (baseline, current) {
        (Some(base), Some(now)) => {
            if now.len() >= base.len() && now.starts_with(base) {
                let delta = &now[base.len()..];
                if delta.is_empty() {
                    fallback.to_string()
                } else {
                    delta.to_string()
                }
            } else if !now.is_empty() && now != base {
                now.to_string()
            } else {
                fallback.to_string()
            }
        }
        _ => fallback.to_string(),
    }
}

#[cfg(windows)]
mod imp {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationValuePattern, UIA_ValuePatternId,
    };

    pub fn focused_text_blocking() -> Option<String> {
        unsafe {
            let init = CoInitializeEx(None, COINIT_MULTITHREADED);
            let should_uninit = init.is_ok();
            let result = read_focused_value();
            if should_uninit {
                CoUninitialize();
            }
            result
        }
    }

    unsafe fn read_focused_value() -> Option<String> {
        unsafe {
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
            let element = automation.GetFocusedElement().ok()?;
            let pattern = element
                .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                .ok()?;
            let value = pattern.CurrentValue().ok()?;
            let text = value.to_string();
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        }
    }
}
