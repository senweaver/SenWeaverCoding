// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub const CLIPBOARD_PREVIEW_CHARS: usize = 120;

#[derive(Debug, Clone)]
pub struct ClipboardSnapshot {
    pub formats: Vec<String>,
    pub length: usize,
    pub hash: String,
    pub text_preview: Option<String>,
}

pub fn collapse_preview(text: &str) -> String {
    let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = collapsed.chars().take(CLIPBOARD_PREVIEW_CHARS).collect();
    if collapsed.chars().count() > CLIPBOARD_PREVIEW_CHARS {
        out.push('…');
    }
    out
}

fn hash_hex16(bytes: &[u8]) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(bytes);
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    hex.chars().take(16).collect()
}

#[cfg(windows)]
pub fn clipboard_sequence_number() -> u32 {
    unsafe { windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber() }
}

#[cfg(not(windows))]
pub fn clipboard_sequence_number() -> u32 {
    0
}

#[cfg(windows)]
pub fn read_clipboard_snapshot() -> Option<ClipboardSnapshot> {
    imp::read_clipboard_snapshot()
}

#[cfg(not(windows))]
pub fn read_clipboard_snapshot() -> Option<ClipboardSnapshot> {
    None
}

#[cfg(windows)]
mod imp {
    use super::{collapse_preview, hash_hex16, ClipboardSnapshot};
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};

    const CF_UNICODETEXT: u32 = 13;
    const CF_HDROP: u32 = 15;
    const CF_DIB: u32 = 8;
    const CF_BITMAP: u32 = 2;
    const MAX_TEXT_CHARS: usize = 200_000;

    struct ClipboardGuard;

    impl ClipboardGuard {
        fn open() -> Option<Self> {
            for _ in 0..2 {
                if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
                    return Some(Self);
                }
                std::thread::sleep(std::time::Duration::from_millis(30));
            }
            None
        }
    }

    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            unsafe { CloseClipboard() };
        }
    }

    fn read_unicode_text() -> Option<String> {
        unsafe {
            let handle = GetClipboardData(CF_UNICODETEXT);
            if handle.is_null() {
                return None;
            }
            let ptr = GlobalLock(handle as _) as *const u16;
            if ptr.is_null() {
                return None;
            }
            let mut len = 0usize;
            while len < MAX_TEXT_CHARS && *ptr.add(len) != 0 {
                len += 1;
            }
            let slice = std::slice::from_raw_parts(ptr, len);
            let text = String::from_utf16_lossy(slice);
            GlobalUnlock(handle as _);
            Some(text)
        }
    }

    fn read_dib_dimensions() -> Option<(i32, i32)> {
        unsafe {
            let handle = GetClipboardData(CF_DIB);
            if handle.is_null() {
                return None;
            }
            let ptr = GlobalLock(handle as _) as *const u8;
            if ptr.is_null() {
                return None;
            }
            let header = std::slice::from_raw_parts(ptr, 16);
            let width = i32::from_le_bytes([header[4], header[5], header[6], header[7]]);
            let height = i32::from_le_bytes([header[8], header[9], header[10], header[11]]);
            GlobalUnlock(handle as _);
            Some((width, height.abs()))
        }
    }

    pub fn read_clipboard_snapshot() -> Option<ClipboardSnapshot> {
        let has_text = unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT) } != 0;
        let has_files = unsafe { IsClipboardFormatAvailable(CF_HDROP) } != 0;
        let has_image = unsafe { IsClipboardFormatAvailable(CF_DIB) } != 0
            || unsafe { IsClipboardFormatAvailable(CF_BITMAP) } != 0;

        let mut formats = Vec::new();
        if has_text {
            formats.push("text".to_string());
        }
        if has_files {
            formats.push("files".to_string());
        }
        if has_image {
            formats.push("image".to_string());
        }
        if formats.is_empty() {
            return None;
        }

        let _guard = ClipboardGuard::open()?;
        if has_text {
            let text = read_unicode_text()?;
            let length = text.chars().count();
            let hash = hash_hex16(text.as_bytes());
            let preview = collapse_preview(&text);
            return Some(ClipboardSnapshot {
                formats,
                length,
                hash,
                text_preview: Some(preview),
            });
        }
        if has_image {
            let dims = read_dib_dimensions();
            let label = match dims {
                Some((w, h)) => format!("[image {w}x{h}]"),
                None => "[image]".to_string(),
            };
            let hash = hash_hex16(label.as_bytes());
            return Some(ClipboardSnapshot {
                formats,
                length: 0,
                hash,
                text_preview: Some(label),
            });
        }
        let hash = hash_hex16(b"[files]");
        Some(ClipboardSnapshot {
            formats,
            length: 0,
            hash,
            text_preview: Some("[files]".to_string()),
        })
    }
}
