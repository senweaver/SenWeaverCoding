// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{anyhow, Result};
use enigo::{
    Axis, Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings,
};

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
        let enigo = new_enigo()?;
        enigo
            .main_display()
            .map_err(|e| anyhow!("failed to query display size: {e}"))
    })
    .await
    .map_err(|e| anyhow!("display query task failed to join: {e}"))?
}

pub async fn move_to(x: i32, y: i32) -> Result<()> {
    run_blocking(move || {
        let mut enigo = new_enigo()?;
        enigo
            .move_mouse(x, y, Coordinate::Abs)
            .map_err(|e| anyhow!("move_mouse failed: {e}"))
    })
    .await
}

pub async fn click(x: i32, y: i32, button: ClickButton, count: u32) -> Result<()> {
    run_blocking(move || {
        let mut enigo = new_enigo()?;
        enigo
            .move_mouse(x, y, Coordinate::Abs)
            .map_err(|e| anyhow!("move_mouse failed: {e}"))?;
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
    run_blocking(move || {
        let mut enigo = new_enigo()?;
        enigo
            .move_mouse(x, y, Coordinate::Abs)
            .map_err(|e| anyhow!("move_mouse failed: {e}"))?;
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
    run_blocking(move || {
        let mut enigo = new_enigo()?;
        enigo
            .move_mouse(from_x, from_y, Coordinate::Abs)
            .map_err(|e| anyhow!("move_mouse failed: {e}"))?;
        std::thread::sleep(std::time::Duration::from_millis(80));
        enigo
            .button(Button::Left, Direction::Press)
            .map_err(|e| anyhow!("button press failed: {e}"))?;
        std::thread::sleep(std::time::Duration::from_millis(80));
        enigo
            .move_mouse(to_x, to_y, Coordinate::Abs)
            .map_err(|e| anyhow!("move_mouse failed: {e}"))?;
        std::thread::sleep(std::time::Duration::from_millis(80));
        enigo
            .button(Button::Left, Direction::Release)
            .map_err(|e| anyhow!("button release failed: {e}"))
    })
    .await
}

async fn run_blocking<F>(f: F) -> Result<()>
where
    F: FnOnce() -> Result<()> + Send + 'static,
{
    tokio::task::spawn_blocking(f)
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
