// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForegroundWindow {
    pub app: String,
    pub stem: String,
    pub title: String,
    pub pid: u32,
    pub path: String,
}

pub fn is_browser_app(process_stem: &str) -> bool {
    matches!(
        process_stem,
        "chrome"
            | "msedge"
            | "firefox"
            | "brave"
            | "opera"
            | "opera_gx"
            | "vivaldi"
            | "arc"
            | "chromium"
            | "yandex"
            | "iron"
            | "thorium"
    )
}

pub fn pretty_app_name(process_stem: &str) -> Option<&'static str> {
    let name = match process_stem {
        "chrome" => "Google Chrome",
        "msedge" => "Microsoft Edge",
        "firefox" => "Mozilla Firefox",
        "brave" => "Brave",
        "opera" | "opera_gx" => "Opera",
        "vivaldi" => "Vivaldi",
        "arc" => "Arc",
        "chromium" => "Chromium",
        "code" => "Visual Studio Code",
        "cursor" => "Cursor",
        "explorer" => "File Explorer",
        "powershell" | "pwsh" => "PowerShell",
        "cmd" => "Command Prompt",
        "windowsterminal" | "wt" => "Windows Terminal",
        "notepad" => "Notepad",
        "excel" => "Microsoft Excel",
        "winword" => "Microsoft Word",
        "powerpnt" => "Microsoft PowerPoint",
        "outlook" | "olk" => "Microsoft Outlook",
        "onenote" => "Microsoft OneNote",
        "teams" | "ms-teams" => "Microsoft Teams",
        "slack" => "Slack",
        "discord" => "Discord",
        "telegram" => "Telegram",
        "wechat" | "weixin" => "WeChat",
        "dingtalk" => "DingTalk",
        "feishu" | "lark" => "Feishu",
        "qq" => "QQ",
        "acrobat" | "acrord32" => "Adobe Acrobat",
        "photoshop" => "Adobe Photoshop",
        "figma" => "Figma",
        "obsidian" => "Obsidian",
        "spotify" => "Spotify",
        "steam" => "Steam",
        _ => return None,
    };
    Some(name)
}

#[cfg(windows)]
pub fn read_foreground_window() -> Option<ForegroundWindow> {
    imp::read_foreground_window()
}

#[cfg(not(windows))]
pub fn read_foreground_window() -> Option<ForegroundWindow> {
    None
}

#[cfg(windows)]
pub fn foreground_process_stem() -> Option<String> {
    imp::foreground_process_stem()
}

#[cfg(not(windows))]
pub fn foreground_process_stem() -> Option<String> {
    None
}

#[cfg(windows)]
mod imp {
    use super::{pretty_app_name, ForegroundWindow};
    use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumChildWindows, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
    };

    fn window_title(hwnd: HWND) -> String {
        let mut buf = [0u16; 512];
        let len = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
        if len <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..len as usize])
    }

    fn window_pid(hwnd: HWND) -> u32 {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        pid
    }

    fn process_image_path(pid: u32) -> Option<String> {
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return None;
            }
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len);
            CloseHandle(handle);
            if ok == 0 || len == 0 {
                return None;
            }
            Some(String::from_utf16_lossy(&buf[..len as usize]))
        }
    }

    fn path_stem(path: &str) -> String {
        std::path::Path::new(path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default()
    }

    struct ChildScan {
        host_pid: u32,
        found: Option<u32>,
        visited: usize,
    }

    unsafe extern "system" fn enum_child_proc(hwnd: HWND, lparam: LPARAM) -> i32 {
        let scan = unsafe { &mut *(lparam as *mut ChildScan) };
        scan.visited += 1;
        if scan.visited > 256 {
            return 0;
        }
        let pid = window_pid(hwnd);
        if pid != 0 && pid != scan.host_pid {
            scan.found = Some(pid);
            return 0;
        }
        1
    }

    fn unwrap_uwp_host(hwnd: HWND, host_pid: u32) -> Option<u32> {
        let mut scan = ChildScan {
            host_pid,
            found: None,
            visited: 0,
        };
        unsafe {
            EnumChildWindows(
                hwnd,
                Some(enum_child_proc),
                &mut scan as *mut ChildScan as LPARAM,
            );
        }
        scan.found
    }

    pub fn read_foreground_window() -> Option<ForegroundWindow> {
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_null() {
            return None;
        }
        let title = window_title(hwnd);
        let mut pid = window_pid(hwnd);
        if pid == 0 {
            return None;
        }
        let mut path = process_image_path(pid).unwrap_or_default();
        let mut stem = path_stem(&path);
        if stem == "applicationframehost" {
            if let Some(real_pid) = unwrap_uwp_host(hwnd, pid) {
                if let Some(real_path) = process_image_path(real_pid) {
                    pid = real_pid;
                    path = real_path;
                    stem = path_stem(&path);
                }
            }
        }
        if stem.is_empty() {
            return None;
        }
        let app = pretty_app_name(&stem)
            .map(str::to_string)
            .unwrap_or_else(|| stem.clone());
        Some(ForegroundWindow {
            app,
            stem,
            title,
            pid,
            path,
        })
    }

    pub fn foreground_process_stem() -> Option<String> {
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_null() {
            return None;
        }
        let pid = window_pid(hwnd);
        if pid == 0 {
            return None;
        }
        process_image_path(pid).map(|p| path_stem(&p))
    }
}
