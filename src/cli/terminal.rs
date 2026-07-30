// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::OnceLock;

static ANSI_SUPPORTED: OnceLock<bool> = OnceLock::new();

pub fn ansi_supported() -> bool {
    *ANSI_SUPPORTED.get_or_init(detect_and_enable_ansi)
}

#[cfg(windows)]
fn detect_and_enable_ansi() -> bool {
    use windows_sys::Win32::System::Console::{
        ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetStdHandle, STD_ERROR_HANDLE,
        STD_OUTPUT_HANDLE, SetConsoleMode,
    };

    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }

    let mut any_enabled = false;
    for std_handle in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        unsafe {
            let handle = GetStdHandle(std_handle);
            if handle.is_null() || handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
                continue;
            }
            let mut mode: u32 = 0;
            if GetConsoleMode(handle, &mut mode) == 0 {
                if std_handle == STD_OUTPUT_HANDLE {
                    any_enabled = true;
                }
                continue;
            }
            if mode & ENABLE_VIRTUAL_TERMINAL_PROCESSING != 0 {
                any_enabled = true;
                continue;
            }
            if SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0 {
                any_enabled = true;
            }
        }
    }
    any_enabled
}

#[cfg(not(windows))]
fn detect_and_enable_ansi() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    match std::env::var("TERM") {
        Ok(term) => term != "dumb",
        Err(_) => true,
    }
}

pub fn paint(code: &str, text: &str) -> String {
    if ansi_supported() {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

pub fn clear_screen_sequence() -> &'static str {
    if ansi_supported() {
        "\x1b[2J\x1b[H"
    } else {
        "\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n"
    }
}
