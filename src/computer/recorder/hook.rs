// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

#[cfg(not(windows))]
use super::types::RawInputEvent;
#[cfg(not(windows))]
use tokio::sync::mpsc::UnboundedReceiver;

#[cfg(windows)]
mod imp {
    use super::super::types::{MouseButton, RawInputEvent};
    use anyhow::{bail, Result};
    use once_cell::sync::OnceCell;
    use parking_lot::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

    use windows_sys::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcessId, GetCurrentThread, GetCurrentThreadId, SetThreadPriority,
        THREAD_PRIORITY_TIME_CRITICAL,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, GetKeyboardLayout, ToUnicodeEx,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetForegroundWindow, GetMessageW,
        GetWindowThreadProcessId, KillTimer, PostThreadMessageW, SetTimer, SetWindowsHookExW,
        TranslateMessage, UnhookWindowsHookEx, WindowFromPoint, HHOOK, HC_ACTION,
        KBDLLHOOKSTRUCT, MSG, MSLLHOOKSTRUCT, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN,
        WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE,
        WM_MOUSEWHEEL, WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN, WM_TIMER,
    };

    const VK_SHIFT: u16 = 0x10;
    const VK_CONTROL: u16 = 0x11;
    const VK_MENU: u16 = 0x12;
    const VK_CAPITAL: u16 = 0x14;
    const VK_LWIN: u16 = 0x5B;
    const VK_RWIN: u16 = 0x5C;
    const VK_LSHIFT: u16 = 0xA0;
    const VK_RSHIFT: u16 = 0xA1;
    const VK_LCONTROL: u16 = 0xA2;
    const VK_RCONTROL: u16 = 0xA3;
    const VK_LMENU: u16 = 0xA4;
    const VK_RMENU: u16 = 0xA5;

    const REHOOK_TIMER_ID: usize = 0x5EA1;
    const REHOOK_INTERVAL_MS: u32 = 5_000;

    static SENDER: OnceCell<Mutex<Option<UnboundedSender<RawInputEvent>>>> = OnceCell::new();
    static SHIFT_DOWN: AtomicBool = AtomicBool::new(false);
    static CTRL_DOWN: AtomicBool = AtomicBool::new(false);
    static ALT_DOWN: AtomicBool = AtomicBool::new(false);
    static WIN_DOWN: AtomicBool = AtomicBool::new(false);
    static CAPS_ON: AtomicBool = AtomicBool::new(false);
    static CAPS_KEY_HELD: AtomicBool = AtomicBool::new(false);
    static LEFT_DOWN: AtomicBool = AtomicBool::new(false);

    fn sender_slot() -> &'static Mutex<Option<UnboundedSender<RawInputEvent>>> {
        SENDER.get_or_init(|| Mutex::new(None))
    }

    fn emit(event: RawInputEvent) {
        if let Some(tx) = sender_slot().lock().as_ref() {
            let _ = tx.send(event);
        }
    }

    fn is_modifier(vk: u16) -> bool {
        matches!(
            vk,
            VK_SHIFT
                | VK_CONTROL
                | VK_MENU
                | VK_LWIN
                | VK_RWIN
                | VK_LSHIFT
                | VK_RSHIFT
                | VK_LCONTROL
                | VK_RCONTROL
                | VK_LMENU
                | VK_RMENU
                | VK_CAPITAL
        )
    }

    fn update_modifiers(vk: u16, down: bool) {
        match vk {
            VK_SHIFT | VK_LSHIFT | VK_RSHIFT => SHIFT_DOWN.store(down, Ordering::Relaxed),
            VK_CONTROL | VK_LCONTROL | VK_RCONTROL => CTRL_DOWN.store(down, Ordering::Relaxed),
            VK_MENU | VK_LMENU | VK_RMENU => ALT_DOWN.store(down, Ordering::Relaxed),
            VK_LWIN | VK_RWIN => WIN_DOWN.store(down, Ordering::Relaxed),
            VK_CAPITAL => {
                if down {
                    if !CAPS_KEY_HELD.swap(true, Ordering::Relaxed) {
                        CAPS_ON.fetch_xor(true, Ordering::Relaxed);
                    }
                } else {
                    CAPS_KEY_HELD.store(false, Ordering::Relaxed);
                }
            }
            _ => {}
        }
    }

    pub fn point_in_own_process(x: i32, y: i32) -> bool {
        unsafe {
            let hwnd = WindowFromPoint(POINT { x, y });
            if hwnd.is_null() {
                return false;
            }
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            pid == GetCurrentProcessId()
        }
    }

    pub fn translate_vk(vk: u16, scan: u32, shift: bool, caps: bool) -> Option<char> {
        let mut keystate = [0u8; 256];
        if shift {
            keystate[VK_SHIFT as usize] = 0x80;
        }
        if caps {
            keystate[VK_CAPITAL as usize] = 0x01;
        }
        let mut buf = [0u16; 8];
        let n = unsafe {
            let fg = GetForegroundWindow();
            let tid = GetWindowThreadProcessId(fg, std::ptr::null_mut());
            let layout = GetKeyboardLayout(tid);
            ToUnicodeEx(
                vk as u32,
                scan,
                keystate.as_ptr(),
                buf.as_mut_ptr(),
                buf.len() as i32,
                0x4,
                layout,
            )
        };
        if n == 1 {
            let c = char::from_u32(buf[0] as u32)?;
            if c.is_control() {
                None
            } else {
                Some(c)
            }
        } else {
            None
        }
    }

    unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 {
            let kb = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
            let vk = kb.vkCode as u16;
            let msg = wparam as u32;
            let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
            update_modifiers(vk, down);
            if !is_modifier(vk) {
                emit(RawInputEvent::Key {
                    down,
                    vk,
                    scan: kb.scanCode,
                    ctrl: CTRL_DOWN.load(Ordering::Relaxed),
                    alt: ALT_DOWN.load(Ordering::Relaxed),
                    shift: SHIFT_DOWN.load(Ordering::Relaxed),
                    win: WIN_DOWN.load(Ordering::Relaxed),
                    caps: CAPS_ON.load(Ordering::Relaxed),
                });
            }
        }
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
    }

    unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 {
            let ms = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
            let x = ms.pt.x;
            let y = ms.pt.y;
            let msg = wparam as u32;
            match msg {
                WM_LBUTTONDOWN => {
                    LEFT_DOWN.store(true, Ordering::Relaxed);
                    emit(RawInputEvent::MouseButton {
                        button: MouseButton::Left,
                        down: true,
                        x,
                        y,
                    });
                }
                WM_LBUTTONUP => {
                    LEFT_DOWN.store(false, Ordering::Relaxed);
                    emit(RawInputEvent::MouseButton {
                        button: MouseButton::Left,
                        down: false,
                        x,
                        y,
                    });
                }
                WM_RBUTTONDOWN => emit(RawInputEvent::MouseButton {
                    button: MouseButton::Right,
                    down: true,
                    x,
                    y,
                }),
                WM_RBUTTONUP => emit(RawInputEvent::MouseButton {
                    button: MouseButton::Right,
                    down: false,
                    x,
                    y,
                }),
                WM_MBUTTONDOWN => emit(RawInputEvent::MouseButton {
                    button: MouseButton::Middle,
                    down: true,
                    x,
                    y,
                }),
                WM_MBUTTONUP => emit(RawInputEvent::MouseButton {
                    button: MouseButton::Middle,
                    down: false,
                    x,
                    y,
                }),
                WM_MOUSEWHEEL => {
                    let delta = ((ms.mouseData >> 16) & 0xffff) as i16 as i32;
                    emit(RawInputEvent::Wheel {
                        delta,
                        horizontal: false,
                        x,
                        y,
                    });
                }
                WM_MOUSEHWHEEL => {
                    let delta = ((ms.mouseData >> 16) & 0xffff) as i16 as i32;
                    emit(RawInputEvent::Wheel {
                        delta,
                        horizontal: true,
                        x,
                        y,
                    });
                }
                WM_MOUSEMOVE => {
                    if LEFT_DOWN.load(Ordering::Relaxed) {
                        emit(RawInputEvent::MouseMove { x, y });
                    }
                }
                _ => {}
            }
        }
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
    }

    struct HookPair {
        keyboard: HHOOK,
        mouse: HHOOK,
    }

    unsafe fn install_hooks() -> Result<HookPair> {
        let kb = unsafe {
            SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), std::ptr::null_mut(), 0)
        };
        let ms =
            unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), std::ptr::null_mut(), 0) };
        if kb.is_null() || ms.is_null() {
            if !kb.is_null() {
                unsafe { UnhookWindowsHookEx(kb) };
            }
            if !ms.is_null() {
                unsafe { UnhookWindowsHookEx(ms) };
            }
            let which = if kb.is_null() && ms.is_null() {
                "keyboard and mouse hooks"
            } else if kb.is_null() {
                "keyboard hook"
            } else {
                "mouse hook"
            };
            bail!("failed to install Windows {which} (permission denied or session 0)");
        }
        Ok(HookPair {
            keyboard: kb,
            mouse: ms,
        })
    }

    unsafe fn uninstall_hooks(pair: &HookPair) {
        unsafe {
            UnhookWindowsHookEx(pair.keyboard);
            UnhookWindowsHookEx(pair.mouse);
        }
    }

    pub struct HookHandle {
        thread_id: u32,
        join: Option<std::thread::JoinHandle<()>>,
    }

    impl HookHandle {
        pub fn stop(mut self) {
            unsafe {
                PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
            }
            if let Some(join) = self.join.take() {
                let _ = join.join();
            }
        }
    }

    pub fn start_capture() -> Result<(HookHandle, UnboundedReceiver<RawInputEvent>)> {
        let (tx, rx) = unbounded_channel();
        SHIFT_DOWN.store(false, Ordering::Relaxed);
        CTRL_DOWN.store(false, Ordering::Relaxed);
        ALT_DOWN.store(false, Ordering::Relaxed);
        WIN_DOWN.store(false, Ordering::Relaxed);
        CAPS_KEY_HELD.store(false, Ordering::Relaxed);
        LEFT_DOWN.store(false, Ordering::Relaxed);
        *sender_slot().lock() = Some(tx);

        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<(Option<String>, u32)>();
        let join = std::thread::Builder::new()
            .name("sen-input-recorder".into())
            .spawn(move || unsafe {
                SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
                let tid = GetCurrentThreadId();
                CAPS_ON.store(
                    (GetKeyState(VK_CAPITAL as i32) & 1) != 0,
                    Ordering::Relaxed,
                );
                let mut hooks = match install_hooks() {
                    Ok(pair) => {
                        let _ = ready_tx.send((None, tid));
                        pair
                    }
                    Err(e) => {
                        let _ = ready_tx.send((Some(e.to_string()), tid));
                        *sender_slot().lock() = None;
                        return;
                    }
                };
                let timer =
                    SetTimer(std::ptr::null_mut(), REHOOK_TIMER_ID, REHOOK_INTERVAL_MS, None);

                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                    if msg.message == WM_TIMER && timer != 0 && msg.wParam == timer {
                        uninstall_hooks(&hooks);
                        match install_hooks() {
                            Ok(pair) => hooks = pair,
                            Err(_) => break,
                        }
                        continue;
                    }
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                if timer != 0 {
                    KillTimer(std::ptr::null_mut(), timer);
                }
                uninstall_hooks(&hooks);
                *sender_slot().lock() = None;
            })?;

        match ready_rx.recv() {
            Ok((None, tid)) => Ok((
                HookHandle {
                    thread_id: tid,
                    join: Some(join),
                },
                rx,
            )),
            Ok((Some(error), _)) => {
                let _ = join.join();
                *sender_slot().lock() = None;
                bail!("{error}")
            }
            Err(_) => {
                *sender_slot().lock() = None;
                bail!("input recorder thread failed to start")
            }
        }
    }
}

#[cfg(windows)]
pub use imp::{point_in_own_process, start_capture, translate_vk, HookHandle};

#[cfg(not(windows))]
pub struct HookHandle;

#[cfg(not(windows))]
impl HookHandle {
    pub fn stop(self) {}
}

#[cfg(not(windows))]
pub fn start_capture() -> anyhow::Result<(HookHandle, UnboundedReceiver<RawInputEvent>)> {
    anyhow::bail!("operation recording is only supported on Windows")
}

#[cfg(not(windows))]
pub fn point_in_own_process(_x: i32, _y: i32) -> bool {
    false
}

#[cfg(not(windows))]
pub fn translate_vk(_vk: u16, _scan: u32, _shift: bool, _caps: bool) -> Option<char> {
    None
}
