// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

const URL_READ_TIMEOUT_MS: u64 = 800;

pub async fn read_browser_url() -> Option<String> {
    let handle = tokio::task::spawn_blocking(read_browser_url_blocking);
    match tokio::time::timeout(
        std::time::Duration::from_millis(URL_READ_TIMEOUT_MS),
        handle,
    )
    .await
    {
        Ok(join) => join.ok().flatten(),
        Err(_) => None,
    }
}

pub fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .split('@')
        .next_back()
        .unwrap_or("");
    let host = host.split(':').next().unwrap_or("");
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

fn normalize_omnibox_value(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 4096 {
        return None;
    }
    if trimmed.contains("://") {
        return Some(trimmed.to_string());
    }
    if trimmed.contains(' ') {
        return None;
    }
    if !trimmed.contains('.') {
        return None;
    }
    Some(format!("https://{trimmed}"))
}

#[cfg(windows)]
fn read_browser_url_blocking() -> Option<String> {
    imp::read_browser_url_blocking()
}

#[cfg(not(windows))]
fn read_browser_url_blocking() -> Option<String> {
    None
}

#[cfg(windows)]
mod imp {
    use super::normalize_omnibox_value;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationValuePattern,
        UIA_ValuePatternId, UIA_CONTROLTYPE_ID,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    const CONTROL_TYPE_EDIT: i32 = 50004;
    const CONTROL_TYPE_DOCUMENT: i32 = 50030;
    const MAX_NODES: usize = 600;
    const MAX_DEPTH: usize = 8;

    pub fn read_browser_url_blocking() -> Option<String> {
        unsafe {
            let init = CoInitializeEx(None, COINIT_MULTITHREADED);
            let should_uninit = init.is_ok();
            let result = walk_foreground_for_url();
            if should_uninit {
                CoUninitialize();
            }
            result
        }
    }

    unsafe fn walk_foreground_for_url() -> Option<String> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_null() {
                return None;
            }
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
            let root = automation
                .ElementFromHandle(HWND(hwnd as *mut _))
                .ok()?;
            let walker = automation.ControlViewWalker().ok()?;

            let mut queue: std::collections::VecDeque<(IUIAutomationElement, usize)> =
                std::collections::VecDeque::new();
            queue.push_back((root, 0));
            let mut visited = 0usize;

            while let Some((element, depth)) = queue.pop_front() {
                visited += 1;
                if visited > MAX_NODES {
                    break;
                }
                let control_type = element
                    .CurrentControlType()
                    .unwrap_or(UIA_CONTROLTYPE_ID(0))
                    .0;
                if control_type == CONTROL_TYPE_EDIT {
                    if let Some(url) = try_read_omnibox(&element) {
                        return Some(url);
                    }
                }
                if control_type == CONTROL_TYPE_DOCUMENT || depth >= MAX_DEPTH {
                    continue;
                }
                let mut child = walker.GetFirstChildElement(&element).ok();
                while let Some(current) = child {
                    queue.push_back((current.clone(), depth + 1));
                    if queue.len() + visited > MAX_NODES {
                        break;
                    }
                    child = walker.GetNextSiblingElement(&current).ok();
                }
            }
            None
        }
    }

    unsafe fn try_read_omnibox(element: &IUIAutomationElement) -> Option<String> {
        unsafe {
            let automation_id = element
                .CurrentAutomationId()
                .map(|b| b.to_string())
                .unwrap_or_default();
            let id_match = matches!(
                automation_id.as_str(),
                "omnibox" | "addressEditBox" | "urlbar-input" | "url_bar" | "addressBarEdit"
            );
            let name_match = if id_match {
                true
            } else {
                let name = element
                    .CurrentName()
                    .map(|b| b.to_string().to_ascii_lowercase())
                    .unwrap_or_default();
                name.contains("address") || name.contains("search or enter")
            };
            if !name_match {
                return None;
            }
            let pattern = element
                .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                .ok()?;
            let value = pattern.CurrentValue().ok()?.to_string();
            normalize_omnibox_value(&value)
        }
    }
}
