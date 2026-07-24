// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{anyhow, Result};
use enigo::{
    Axis, Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings,
};

#[cfg(windows)]
mod winmouse {
    use anyhow::{anyhow, Result};
    use windows_sys::Win32::Foundation::{HWND, POINT};
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_MOVE,
        MOUSEEVENTF_VIRTUALDESK,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetAncestor, GetSystemMetrics, GetWindowLongPtrW, GetWindowThreadProcessId,
        SetWindowLongPtrW, WindowFromPoint, GA_ROOT, GWL_EXSTYLE, SM_CXVIRTUALSCREEN,
        SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, WS_EX_TRANSPARENT,
    };

    pub struct OwnTransparency {
        restore: Vec<(HWND, isize)>,
    }

    impl OwnTransparency {
        pub fn for_points(points: &[(i32, i32)]) -> Self {
            let mut restore: Vec<(HWND, isize)> = Vec::new();
            let own_pid = unsafe { GetCurrentProcessId() };
            for &(x, y) in points {
                unsafe {
                    let hwnd = WindowFromPoint(POINT { x, y });
                    if hwnd.is_null() {
                        continue;
                    }
                    let root = GetAncestor(hwnd, GA_ROOT);
                    let target = if root.is_null() { hwnd } else { root };
                    let mut pid: u32 = 0;
                    GetWindowThreadProcessId(target, &mut pid);
                    if pid != own_pid {
                        continue;
                    }
                    if restore.iter().any(|(h, _)| *h == target) {
                        continue;
                    }
                    let old = GetWindowLongPtrW(target, GWL_EXSTYLE);
                    let want = old | (WS_EX_TRANSPARENT as isize);
                    if want != old {
                        SetWindowLongPtrW(target, GWL_EXSTYLE, want);
                        restore.push((target, old));
                    }
                }
            }
            Self { restore }
        }
    }

    impl Drop for OwnTransparency {
        fn drop(&mut self) {
            for (hwnd, old) in self.restore.drain(..) {
                unsafe {
                    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, old);
                }
            }
        }
    }

    fn virtual_bounds() -> (i32, i32, i32, i32) {
        unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        }
    }

    pub fn move_absolute(x: i32, y: i32) -> Result<()> {
        let (vx, vy, vw, vh) = virtual_bounds();
        if vw <= 1 || vh <= 1 {
            return Err(anyhow!("virtual screen metrics unavailable"));
        }
        let rel_x = (x - vx).clamp(0, vw - 1);
        let rel_y = (y - vy).clamp(0, vh - 1);
        let norm_x = (i64::from(rel_x) * 65535 + i64::from(vw - 1) / 2) / i64::from(vw - 1);
        let norm_y = (i64::from(rel_y) * 65535 + i64::from(vh - 1) / 2) / i64::from(vh - 1);

        let mut input: INPUT = unsafe { std::mem::zeroed() };
        input.r#type = INPUT_MOUSE;
        input.Anonymous.mi.dx = norm_x as i32;
        input.Anonymous.mi.dy = norm_y as i32;
        input.Anonymous.mi.dwFlags =
            MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK;
        let sent = unsafe {
            SendInput(1, &input as *const INPUT, std::mem::size_of::<INPUT>() as i32)
        };
        if sent == 1 {
            Ok(())
        } else {
            Err(anyhow!("SendInput move failed"))
        }
    }
}

#[cfg(windows)]
async fn move_absolute(x: i32, y: i32) -> Result<()> {
    run_blocking(move || {
        winmouse::move_absolute(x, y)?;
        std::thread::sleep(std::time::Duration::from_millis(8));
        Ok(())
    })
    .await
}

#[cfg(not(windows))]
async fn move_absolute(_x: i32, _y: i32) -> Result<()> {
    Err(anyhow!("virtual desktop mouse move is only supported on Windows"))
}

#[cfg(windows)]
fn own_transparency(points: &[(i32, i32)]) -> winmouse::OwnTransparency {
    winmouse::OwnTransparency::for_points(points)
}

#[cfg(not(windows))]
fn own_transparency(_points: &[(i32, i32)]) {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickButton {
    Left,
    Right,
    Middle,
}

impl ClickButton {
    fn to_enigo(self) -> Button {
        match self {
            ClickButton::Left => Button::Left,
            ClickButton::Right => Button::Right,
            ClickButton::Middle => Button::Middle,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDirection {
    Up,
    Down,
    Left,
    Right,
}

fn new_enigo() -> Result<Enigo> {
    Enigo::new(&Settings::default()).map_err(|e| anyhow!("failed to initialize input backend: {e}"))
}

pub async fn main_display_size() -> Result<(i32, i32)> {
    tokio::task::spawn_blocking(|| {
        super::super::dpi::ensure_dpi_awareness();
        let enigo = new_enigo()?;
        enigo
            .main_display()
            .map_err(|e| anyhow!("failed to query display size: {e}"))
    })
    .await
    .map_err(|e| anyhow!("display query task failed to join: {e}"))?
}

pub async fn move_to(x: i32, y: i32) -> Result<()> {
    if move_absolute(x, y).await.is_ok() {
        return Ok(());
    }
    run_blocking(move || {
        let mut enigo = new_enigo()?;
        enigo
            .move_mouse(x, y, Coordinate::Abs)
            .map_err(|e| anyhow!("move_mouse failed: {e}"))
    })
    .await
}

pub async fn click(x: i32, y: i32, button: ClickButton, count: u32) -> Result<()> {
    let positioned = move_absolute(x, y).await.is_ok();
    run_blocking(move || {
        let _guard = own_transparency(&[(x, y)]);
        let mut enigo = new_enigo()?;
        if !positioned {
            enigo
                .move_mouse(x, y, Coordinate::Abs)
                .map_err(|e| anyhow!("move_mouse failed: {e}"))?;
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
        let btn = button.to_enigo();
        for _ in 0..count.max(1) {
            enigo
                .button(btn, Direction::Click)
                .map_err(|e| anyhow!("button click failed: {e}"))?;
            std::thread::sleep(std::time::Duration::from_millis(40));
        }
        Ok(())
    })
    .await
}

pub async fn type_text(text: String) -> Result<()> {
    run_blocking(move || {
        let mut enigo = new_enigo()?;
        enigo
            .text(&text)
            .map_err(|e| anyhow!("text input failed: {e}"))
    })
    .await
}

pub async fn key_combo(combo: String) -> Result<()> {
    run_blocking(move || {
        let mut enigo = new_enigo()?;
        let (modifiers, key) = parse_combo(&combo)?;
        for m in &modifiers {
            enigo
                .key(*m, Direction::Press)
                .map_err(|e| anyhow!("modifier press failed: {e}"))?;
        }
        let result = enigo.key(key, Direction::Click);
        for m in modifiers.iter().rev() {
            let _ = enigo.key(*m, Direction::Release);
        }
        result.map_err(|e| anyhow!("key press failed: {e}"))
    })
    .await
}

pub async fn scroll(x: i32, y: i32, direction: ScrollDirection, amount: i32) -> Result<()> {
    let positioned = move_absolute(x, y).await.is_ok();
    run_blocking(move || {
        let _guard = own_transparency(&[(x, y)]);
        let mut enigo = new_enigo()?;
        if !positioned {
            enigo
                .move_mouse(x, y, Coordinate::Abs)
                .map_err(|e| anyhow!("move_mouse failed: {e}"))?;
        }
        let magnitude = amount.max(1);
        let (axis, length) = match direction {
            ScrollDirection::Down => (Axis::Vertical, magnitude),
            ScrollDirection::Up => (Axis::Vertical, -magnitude),
            ScrollDirection::Right => (Axis::Horizontal, magnitude),
            ScrollDirection::Left => (Axis::Horizontal, -magnitude),
        };
        enigo
            .scroll(length, axis)
            .map_err(|e| anyhow!("scroll failed: {e}"))
    })
    .await
}

pub async fn drag(from_x: i32, from_y: i32, to_x: i32, to_y: i32) -> Result<()> {
    let use_virtual = move_absolute(from_x, from_y).await.is_ok();
    run_blocking(move || {
        let _guard = own_transparency(&[(from_x, from_y), (to_x, to_y)]);
        let mut enigo = new_enigo()?;
        if !use_virtual {
            enigo
                .move_mouse(from_x, from_y, Coordinate::Abs)
                .map_err(|e| anyhow!("move_mouse failed: {e}"))?;
        } else {
            winmouse_move(from_x, from_y)?;
        }
        std::thread::sleep(std::time::Duration::from_millis(80));
        enigo
            .button(Button::Left, Direction::Press)
            .map_err(|e| anyhow!("button press failed: {e}"))?;
        std::thread::sleep(std::time::Duration::from_millis(80));
        if use_virtual {
            winmouse_move(to_x, to_y)?;
        } else {
            enigo
                .move_mouse(to_x, to_y, Coordinate::Abs)
                .map_err(|e| anyhow!("move_mouse failed: {e}"))?;
        }
        std::thread::sleep(std::time::Duration::from_millis(80));
        enigo
            .button(Button::Left, Direction::Release)
            .map_err(|e| anyhow!("button release failed: {e}"))
    })
    .await
}

#[cfg(windows)]
fn winmouse_move(x: i32, y: i32) -> Result<()> {
    winmouse::move_absolute(x, y)?;
    std::thread::sleep(std::time::Duration::from_millis(8));
    Ok(())
}

#[cfg(not(windows))]
fn winmouse_move(_x: i32, _y: i32) -> Result<()> {
    Err(anyhow!("virtual desktop move is only supported on Windows"))
}

async fn run_blocking<F>(f: F) -> Result<()>
where
    F: FnOnce() -> Result<()> + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        super::super::dpi::ensure_dpi_awareness();
        f()
    })
    .await
    .map_err(|e| anyhow!("input task failed to join: {e}"))?
}

fn parse_combo(combo: &str) -> Result<(Vec<Key>, Key)> {
    let parts: Vec<&str> = combo
        .split('+')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return Err(anyhow!("empty key combination"));
    }

    let mut modifiers = Vec::new();
    let mut key: Option<Key> = None;
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => modifiers.push(Key::Control),
            "alt" | "option" => modifiers.push(Key::Alt),
            "shift" => modifiers.push(Key::Shift),
            "cmd" | "command" | "super" | "meta" | "win" | "windows" => modifiers.push(Key::Meta),
            other => key = Some(map_key(other)?),
        }
    }

    let key = key.ok_or_else(|| anyhow!("key combination '{combo}' has no primary key"))?;
    Ok((modifiers, key))
}

fn map_key(name: &str) -> Result<Key> {
    let key = match name {
        "enter" | "return" => Key::Return,
        "tab" => Key::Tab,
        "esc" | "escape" => Key::Escape,
        "space" | "spacebar" => Key::Space,
        "backspace" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "up" | "arrowup" => Key::UpArrow,
        "down" | "arrowdown" => Key::DownArrow,
        "left" | "arrowleft" => Key::LeftArrow,
        "right" | "arrowright" => Key::RightArrow,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" | "pgup" => Key::PageUp,
        "pagedown" | "pgdn" => Key::PageDown,
        "f1" => Key::F1,
        "f2" => Key::F2,
        "f3" => Key::F3,
        "f4" => Key::F4,
        "f5" => Key::F5,
        "f6" => Key::F6,
        "f7" => Key::F7,
        "f8" => Key::F8,
        "f9" => Key::F9,
        "f10" => Key::F10,
        "f11" => Key::F11,
        "f12" => Key::F12,
        other => {
            let mut chars = other.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Key::Unicode(c),
                _ => return Err(anyhow!("unsupported key name: {other}")),
            }
        }
    };
    Ok(key)
}
