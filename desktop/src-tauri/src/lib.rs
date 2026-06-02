// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

mod bootstrap_diag;
mod browser_dock;
mod fetch_worker;
mod process_lifetime;
mod terminal;

use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    panic::AssertUnwindSafe,
    path::PathBuf,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};

use browser_dock::DockSharedState;
use terminal::TerminalState;

const EMBEDDED_GATEWAY_PENDING_MSG: &str = "desktop server is starting";
const GATEWAY_HEALTH_DEADLINE_SECS: u64 = 90;
const GATEWAY_HEALTH_PROBE_INTERVAL_MS: u64 = 250;
const GATEWAY_HEALTH_PROBE_TIMEOUT_MS: u64 = 2_000;
const GATEWAY_DIAGNOSTIC_HINT_AFTER_SECS: u64 = 30;
const RESTART_DEBOUNCE_SECS: u64 = 45;
const HEALTH_PROBE_HEADER: &str = "X-Sen-Ping";
const HEALTH_PROBE_HEADER_VALUE: &str = "1";
const BACKEND_STATE_EVENT: &str = "backend://state-change";

fn adapters_restart_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

#[cfg(target_os = "windows")]
fn reapply_chrome_styles(hwnd: windows_sys::Win32::Foundation::HWND) {
    use std::ptr;
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR};
    use windows_sys::Win32::Graphics::Gdi::InvalidateRect;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_STYLE, HWND_TOP, SWP_FRAMECHANGED,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_BORDER, WS_CAPTION, WS_DLGFRAME,
        WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
    };

    const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;

    if hwnd.is_null() {
        return;
    }

    let chrome_style_mask: isize = (WS_CAPTION | WS_BORDER | WS_DLGFRAME) as isize;
    let required_style_bits: isize =
        (WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_THICKFRAME) as isize;
    let prev_style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) };
    let new_style = (prev_style & !chrome_style_mask) | required_style_bits;
    if new_style != prev_style {
        unsafe {
            SetWindowLongPtrW(hwnd, GWL_STYLE, new_style);
        }
    }

    let value: u32 = DWMWA_COLOR_NONE;
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR as u32,
            (&value as *const u32).cast(),
            std::mem::size_of::<u32>() as u32,
        );
        SetWindowPos(
            hwnd,
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
        InvalidateRect(hwnd, ptr::null(), 1);
    }
}

#[cfg(target_os = "windows")]
fn disable_window_focus_border(window: &tauri::WebviewWindow) {
    use std::ptr;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR};
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, InvalidateRect, MonitorFromWindow, RedrawWindow, MONITORINFO,
        MONITOR_DEFAULTTONEAREST, RDW_FRAME, RDW_INVALIDATE,
    };
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, GetWindowLongPtrW, GetWindowRect, IsZoomed, SetWindowLongPtrW,
        SetWindowPos, GWL_STYLE, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCLIENT, HTLEFT, HTRIGHT,
        HTTOP, HTTOPLEFT, HTTOPRIGHT, HWND_TOP, SM_CXPADDEDBORDER, SM_CXSIZEFRAME, SWP_FRAMECHANGED,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WM_ACTIVATE, WM_ACTIVATEAPP,
        WM_DPICHANGED, WM_DWMCOMPOSITIONCHANGED, WM_DWMNCRENDERINGCHANGED, WM_GETMINMAXINFO,
        WM_NCACTIVATE, WM_NCCALCSIZE, WM_NCHITTEST, WM_NCPAINT, WM_SETFOCUS, WM_SETTINGCHANGE,
        WM_SHOWWINDOW, WM_THEMECHANGED, WM_WINDOWPOSCHANGED, MINMAXINFO, NCCALCSIZE_PARAMS,
        WS_BORDER, WS_CAPTION, WS_DLGFRAME, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU,
        WS_THICKFRAME,
    };

    const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;
    const SUBCLASS_ID: usize = 0x53_45_4E_57;
    const WM_NCUAHDRAWCAPTION: u32 = 0x00AE;
    const WM_NCUAHDRAWFRAME: u32 = 0x00AF;

    unsafe extern "system" fn no_border_subclass(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _uid: usize,
        _data: usize,
    ) -> LRESULT {
        match msg {
            WM_GETMINMAXINFO => {
                unsafe {
                    let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                    if !monitor.is_null() {
                        let mut mi: MONITORINFO = std::mem::zeroed();
                        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
                        if GetMonitorInfoW(monitor, &mut mi) != 0 {
                            let work = mi.rcWork;
                            let mon = mi.rcMonitor;
                            let mmi = &mut *(lparam as *mut MINMAXINFO);
                            mmi.ptMaxPosition.x = work.left - mon.left;
                            mmi.ptMaxPosition.y = work.top - mon.top;
                            mmi.ptMaxSize.x = work.right - work.left;
                            mmi.ptMaxSize.y = work.bottom - work.top;
                            mmi.ptMaxTrackSize.x = work.right - work.left;
                            mmi.ptMaxTrackSize.y = work.bottom - work.top;
                        }
                    }
                }
                0
            }
            WM_NCCALCSIZE => {
                if wparam != 0 && unsafe { IsZoomed(hwnd) } != 0 {
                    unsafe {
                        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                        if !monitor.is_null() {
                            let mut mi: MONITORINFO = std::mem::zeroed();
                            mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
                            if GetMonitorInfoW(monitor, &mut mi) != 0 {
                                let params = &mut *(lparam as *mut NCCALCSIZE_PARAMS);
                                params.rgrc[0] = mi.rcWork;
                            }
                        }
                    }
                }
                0
            }
            WM_NCPAINT => 0,
            WM_NCUAHDRAWCAPTION | WM_NCUAHDRAWFRAME => 0,

            WM_NCACTIVATE => unsafe { DefSubclassProc(hwnd, msg, wparam, -1) },

            WM_NCHITTEST => {
                if unsafe { IsZoomed(hwnd) } != 0 {
                    return HTCLIENT as LRESULT;
                }

                let cursor_x = ((lparam as u32) & 0xFFFF) as i16 as i32;
                let cursor_y = (((lparam as u32) >> 16) & 0xFFFF) as i16 as i32;

                let mut rect = RECT {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                };
                if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
                    return HTCLIENT as LRESULT;
                }

                let frame_x = unsafe { GetSystemMetrics(SM_CXSIZEFRAME) };
                let padded = unsafe { GetSystemMetrics(SM_CXPADDEDBORDER) };
                let border = (frame_x + padded).max(8);

                let near_left = cursor_x >= rect.left && cursor_x < rect.left + border;
                let near_right = cursor_x < rect.right && cursor_x >= rect.right - border;
                let near_top = cursor_y >= rect.top && cursor_y < rect.top + border;
                let near_bottom = cursor_y < rect.bottom && cursor_y >= rect.bottom - border;

                let hit = match (near_top, near_bottom, near_left, near_right) {
                    (true, _, true, _) => HTTOPLEFT,
                    (true, _, _, true) => HTTOPRIGHT,
                    (_, true, true, _) => HTBOTTOMLEFT,
                    (_, true, _, true) => HTBOTTOMRIGHT,
                    (true, _, _, _) => HTTOP,
                    (_, true, _, _) => HTBOTTOM,
                    (_, _, true, _) => HTLEFT,
                    (_, _, _, true) => HTRIGHT,
                    _ => HTCLIENT,
                };
                hit as LRESULT
            }

            WM_DWMCOMPOSITIONCHANGED
            | WM_DWMNCRENDERINGCHANGED
            | WM_THEMECHANGED
            | WM_DPICHANGED
            | WM_SETTINGCHANGE => {
                let r = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
                reapply_chrome_styles(hwnd);
                r
            }

            WM_SHOWWINDOW => {
                let r = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
                if wparam != 0 {
                    reapply_chrome_styles(hwnd);
                }
                r
            }

            WM_WINDOWPOSCHANGED => {
                let r = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
                unsafe {
                    InvalidateRect(hwnd, ptr::null(), 0);
                    RedrawWindow(
                        hwnd,
                        ptr::null(),
                        ptr::null_mut(),
                        RDW_FRAME | RDW_INVALIDATE,
                    );
                }
                r
            }

            WM_ACTIVATE | WM_ACTIVATEAPP | WM_SETFOCUS => {
                unsafe {
                    InvalidateRect(hwnd, ptr::null(), 1);
                }
                unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
            }
            _ => unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
        }
    }

    let hwnd = match window.hwnd() {
        Ok(handle) => handle,
        Err(err) => {
            tracing::warn!("[sen-desktop] cannot fetch HWND for border fix: {err}");
            return;
        }
    };
    let raw = hwnd.0 as HWND;

    let chrome_style_mask: isize = (WS_CAPTION | WS_BORDER | WS_DLGFRAME) as isize;
    let required_style_bits: isize =
        (WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_THICKFRAME) as isize;
    let prev_style = unsafe { GetWindowLongPtrW(raw, GWL_STYLE) };
    let new_style = (prev_style & !chrome_style_mask) | required_style_bits;
    if new_style != prev_style {
        unsafe {
            SetWindowLongPtrW(raw, GWL_STYLE, new_style);
        }
        tracing::debug!(
            "[sen-desktop] rewrote chrome styles: 0x{:08X} -> 0x{:08X}",
            prev_style as u32,
            new_style as u32
        );
    }

    let value: u32 = DWMWA_COLOR_NONE;
    let border_hr = unsafe {
        DwmSetWindowAttribute(
            raw,
            DWMWA_BORDER_COLOR as u32,
            (&value as *const u32).cast(),
            std::mem::size_of::<u32>() as u32,
        )
    };
    if border_hr < 0 {
        tracing::debug!(
            "[sen-desktop] DwmSetWindowAttribute(BORDER_COLOR) returned 0x{:08X} \
             (expected on pre-Win11-22H2 systems; subclass fallback covers it)",
            border_hr as u32
        );
    }

    let installed = unsafe { SetWindowSubclass(raw, Some(no_border_subclass), SUBCLASS_ID, 0) };
    if installed == 0 {
        tracing::warn!(
            "[sen-desktop] SetWindowSubclass(no-border) failed; the OS focus border may still be visible"
        );
    }

    unsafe {
        SetWindowPos(
            raw,
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );

        InvalidateRect(raw, ptr::null(), 1);
    }
}

#[derive(Clone)]
pub(crate) struct ServerState(Arc<Mutex<ServerStatus>>);

impl Default for ServerState {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(ServerStatus::default())))
    }
}

#[derive(Default)]
struct ServerStatus {
    url: Option<String>,

    bootstrap_generation: u64,

    bootstrap_in_progress: bool,

    last_bootstrap_started_at: Option<Instant>,

    last_error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapStatusPayload {
    state: &'static str,
    url: Option<String>,
    error: Option<String>,
    elapsed_ms: Option<u64>,
    log_dir: Option<String>,
}

fn snapshot_status_locked(guard: &ServerStatus) -> BootstrapStatusPayload {
    let elapsed_ms = guard
        .last_bootstrap_started_at
        .map(|s| s.elapsed().as_millis() as u64);
    let log_dir = sen_log_dir().map(|p| p.to_string_lossy().into_owned());
    let state_label: &'static str = if guard.url.is_some() {
        "ready"
    } else if guard.bootstrap_in_progress {
        "starting"
    } else if guard.last_error.is_some() {
        "failed"
    } else {
        "pending"
    };
    BootstrapStatusPayload {
        state: state_label,
        url: guard.url.clone(),
        error: guard.last_error.clone(),
        elapsed_ms,
        log_dir,
    }
}

fn emit_backend_state(handle: &AppHandle, state: &ServerState) {
    let payload = {
        let g = state.0.lock();
        snapshot_status_locked(&g)
    };
    if let Err(err) = handle.emit(BACKEND_STATE_EVENT, payload) {
        tracing::debug!("[sen-desktop] failed to emit backend state event: {err}");
    }
}

#[tauri::command]
fn get_server_url(state: State<'_, ServerState>) -> Result<String, String> {
    let guard = state.0.lock();
    if let Some(url) = guard.url.as_ref() {
        return Ok(url.clone());
    }
    Err(EMBEDDED_GATEWAY_PENDING_MSG.to_string())
}

#[tauri::command]
fn get_server_status(state: State<'_, ServerState>) -> BootstrapStatusPayload {
    let guard = state.0.lock();
    snapshot_status_locked(&guard)
}

#[tauri::command]
async fn restart_adapters_sidecar(state: State<'_, ServerState>) -> Result<(), String> {
    let url = {
        let guard = state.0.lock();
        guard
            .url
            .clone()
            .ok_or_else(|| EMBEDDED_GATEWAY_PENDING_MSG.to_string())?
    };
    let resp = adapters_restart_client()
        .post(format!("{url}/api/channels/restart"))
        .header(HEALTH_PROBE_HEADER, HEALTH_PROBE_HEADER_VALUE)
        .send()
        .await
        .map_err(|e| format!("channels restart request failed: {e}"))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!(
            "channels restart returned HTTP {}",
            resp.status().as_u16()
        ))
    }
}

#[tauri::command]
fn restart_embedded_gateway(
    handle: AppHandle,
    state: State<'_, ServerState>,
    force: Option<bool>,
) -> Result<(), String> {
    let force = force.unwrap_or(false);
    {
        let mut guard = state.0.lock();
        if guard.bootstrap_in_progress {
            if !force {
                return Err(
                    "gateway bootstrap is already in progress; ignoring restart request".to_string(),
                );
            }

            guard.bootstrap_generation = guard.bootstrap_generation.saturating_add(1);
            guard.bootstrap_in_progress = false;
            guard.url = None;
            guard.last_error = Some(
                "previous bootstrap forcibly invalidated by restart request".to_string(),
            );
            tracing::warn!(
                "[sen-desktop] force restart requested while bootstrap in progress; invalidating previous generation"
            );
        } else if !force {
            if let Some(started) = guard.last_bootstrap_started_at {
                if started.elapsed() < Duration::from_secs(RESTART_DEBOUNCE_SECS) {
                    return Err(format!(
                        "gateway was (re)started less than {}s ago; refusing duplicate restart",
                        RESTART_DEBOUNCE_SECS
                    ));
                }
            }
        }
    }
    spawn_gateway_bootstrap_thread(handle, state.inner().clone())
}

#[tauri::command]
async fn open_log_dir() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(open_log_dir_blocking)
        .await
        .map_err(|err| format!("open log dir task failed: {err}"))?
}

fn open_log_dir_blocking() -> Result<String, String> {
    let dir = sen_log_dir().ok_or_else(|| "could not resolve sen config directory".to_string())?;
    if !dir.exists() {
        std::fs::create_dir_all(&dir)
            .map_err(|err| format!("create log dir failed: {err}"))?;
    }
    reveal_in_explorer_blocking(dir.to_string_lossy().into_owned())?;
    Ok(dir.to_string_lossy().into_owned())
}

fn sen_log_dir() -> Option<PathBuf> {
    sen_config_dir().map(|p| p.join("logs"))
}

fn sen_config_dir() -> Option<PathBuf> {
    if let Some(custom) = senweavercoding::util::get_runtime_var("SEN_CONFIG_DIR") {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    if let Some(home) = senweavercoding::util::get_runtime_var("HOME") {
        if !home.is_empty() {
            return Some(PathBuf::from(home).join(".senweavercoding"));
        }
    }
    if let Some(userprofile) = senweavercoding::util::get_runtime_var("USERPROFILE") {
        if !userprofile.is_empty() {
            return Some(PathBuf::from(userprofile).join(".senweavercoding"));
        }
    }
    None
}

#[tauri::command]
async fn prepare_for_update_install(handle: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        process_lifetime::run_full_shutdown(&handle, Duration::from_secs(8));
    })
    .await
    .map_err(|err| format!("update shutdown task failed: {err}"))?;
    Ok(())
}

#[tauri::command]
fn signal_frontend_ready(handle: AppHandle) -> Result<(), String> {
    if let Some(main) = handle.get_webview_window("main") {
        show_main_window_now(&main);
    }
    Ok(())
}

const MAIN_WINDOW_SHOW_FALLBACK_MS: u64 = 1_500;

fn show_main_window_now(window: &tauri::WebviewWindow) {
    static SHOWN: parking_lot::Mutex<bool> = parking_lot::Mutex::new(false);
    {
        let mut guard = SHOWN.lock();
        if *guard {
            return;
        }
        *guard = true;
    }

    #[cfg(target_os = "windows")]
    if let Ok(hwnd) = window.hwnd() {
        use windows_sys::Win32::Foundation::HWND;
        let raw = hwnd.0 as HWND;
        reapply_chrome_styles(raw);
    }

    if let Err(err) = window.show() {
        tracing::warn!("[sen-desktop] window.show() failed: {err}");
    }
    if let Err(err) = window.set_focus() {
        tracing::debug!("[sen-desktop] window.set_focus() failed: {err}");
    }

    #[cfg(target_os = "windows")]
    if let Ok(hwnd) = window.hwnd() {
        use windows_sys::Win32::Foundation::HWND;
        let raw = hwnd.0 as HWND;
        reapply_chrome_styles(raw);
    }
}

fn schedule_main_window_show_fallback(window: tauri::WebviewWindow) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(MAIN_WINDOW_SHOW_FALLBACK_MS)).await;
        let win_for_closure = window.clone();
        let _ = window.run_on_main_thread(move || {
            show_main_window_now(&win_for_closure);
        });
    });
}

#[tauri::command]
async fn reveal_in_explorer(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || reveal_in_explorer_blocking(path))
        .await
        .map_err(|err| format!("reveal task failed: {err}"))?
}

fn reveal_in_explorer_blocking(path: String) -> Result<(), String> {
    use std::path::PathBuf;

    let target = PathBuf::from(&path);
    let target_for_open: PathBuf = if target.exists() {
        target.clone()
    } else {
        let parent_exists = target
            .parent()
            .map(|p| p.exists())
            .unwrap_or(false);
        let needs_dir = !parent_exists
            || target.extension().is_none();
        if needs_dir {
            std::fs::create_dir_all(&target)
                .map_err(|e| format!("create dir failed for {}: {e}", target.display()))?;
            target.clone()
        } else {
            target
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or(target.clone())
        }
    };

    let is_file = target_for_open.is_file();

    #[cfg(target_os = "windows")]
    {
        let mut cmd = senweavercoding::util::hidden_sync_command("explorer.exe");
        if is_file {
            cmd.arg(format!("/select,{}", target_for_open.display()));
        } else {
            cmd.arg(target_for_open.as_os_str());
        }
        cmd.spawn()
            .map_err(|e| format!("explorer.exe spawn failed: {e}"))?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        let mut cmd = senweavercoding::util::hidden_sync_command("open");
        if is_file {
            cmd.arg("-R").arg(target_for_open.as_os_str());
        } else {
            cmd.arg(target_for_open.as_os_str());
        }
        cmd.spawn()
            .map_err(|e| format!("open spawn failed: {e}"))?;
        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let dir_to_open = if is_file {
            target_for_open
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or(target_for_open.clone())
        } else {
            target_for_open.clone()
        };
        senweavercoding::util::hidden_sync_command("xdg-open")
            .arg(dir_to_open.as_os_str())
            .spawn()
            .map_err(|e| format!("xdg-open spawn failed: {e}"))?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    Err("unsupported platform".to_string())
}

pub fn run() {
    let log_dir = sen_log_dir();
    bootstrap_diag::install_tracing(log_dir.as_deref());
    tracing::info!(
        log_dir = ?bootstrap_diag::current_log_dir(),
        "[sen-desktop] starting desktop shell"
    );

    process_lifetime::install_kill_on_close_job();

    let builder = tauri::Builder::default()
        .manage(ServerState::default())
        .manage(TerminalState::default())
        .manage(DockSharedState::new())
        .register_uri_scheme_protocol("senbridge", browser_dock::senbridge_protocol_handler)
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            get_server_url,
            get_server_status,
            restart_embedded_gateway,
            restart_adapters_sidecar,
            open_log_dir,
            prepare_for_update_install,
            signal_frontend_ready,
            reveal_in_explorer,
            terminal::terminal_spawn,
            terminal::terminal_write,
            terminal::terminal_resize,
            terminal::terminal_kill,
            browser_dock::browser_dock_open,
            browser_dock::browser_dock_set_rect,
            browser_dock::browser_dock_resync,
            browser_dock::browser_dock_hide,
            browser_dock::browser_dock_park,
            browser_dock::browser_dock_focus_active,
            browser_dock::browser_dock_close,
            browser_dock::browser_dock_navigate,
            browser_dock::browser_dock_back,
            browser_dock::browser_dock_forward,
            browser_dock::browser_dock_reload,
            browser_dock::browser_dock_set_zoom,
            browser_dock::browser_dock_set_pick_mode,
            browser_dock::browser_dock_inspect_selector,
            browser_dock::browser_dock_clear,
            browser_dock::browser_dock_request_state,
            browser_dock::browser_dock_get_state,
            browser_dock::browser_dock_open_devtools,
            browser_dock::browser_dock_close_devtools,
            browser_dock::browser_dock_new_tab,
            browser_dock::browser_dock_close_tab,
            browser_dock::browser_dock_activate_tab,
            browser_dock::browser_dock_list_tabs,
            browser_dock::browser_dock_screenshot,
            browser_dock::browser_dock_pin_test_target,
            browser_dock::browser_dock_clear_test_target,
            browser_dock::browser_dock_get_test_target,
            browser_dock::browser_dock_release_agent_tab_for_session,
            browser_dock::browser_dock_bind_tab_to_session,
            browser_dock::browser_dock_unbind_tab_from_session,
            browser_dock::browser_dock_present_session,
            browser_dock::browser_dock_set_foreground_session,
        ]);

    let app_build = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        builder
        .setup(|app| {
            if let Some(main) = app.get_webview_window("main") {
                if let Err(err) = main.set_resizable(true) {
                    tracing::debug!("[sen-desktop] set_resizable(true) failed: {err}");
                }

                #[cfg(target_os = "windows")]
                disable_window_focus_border(&main);

                schedule_main_window_show_fallback(main.clone());
            }

            browser_dock::install_into(app.handle());
            fetch_worker::install_into(app.handle());

            let handle = app.handle().clone();

            let server_state = app.state::<ServerState>().inner().clone();
            const GATEWAY_SPAWN_MAX_ATTEMPTS: usize = 4;
            const GATEWAY_SPAWN_RETRY_DELAY_MS: u64 = 100;
            let mut spawn_ok = false;
            let mut last_spawn_err = String::new();
            for attempt in 0..GATEWAY_SPAWN_MAX_ATTEMPTS {
                match spawn_gateway_bootstrap_thread(handle.clone(), server_state.clone()) {
                    Ok(()) => {
                        spawn_ok = true;
                        break;
                    }
                    Err(err) => {
                        last_spawn_err = err.clone();
                        tracing::warn!(
                            "[sen-desktop] gateway bootstrap thread spawn attempt {} of {} failed: {err}",
                            attempt + 1,
                            GATEWAY_SPAWN_MAX_ATTEMPTS,
                        );
                        if attempt + 1 < GATEWAY_SPAWN_MAX_ATTEMPTS {
                            thread::sleep(Duration::from_millis(GATEWAY_SPAWN_RETRY_DELAY_MS));
                        }
                    }
                }
            }
            if !spawn_ok {
                let msg = format!(
                    "could not spawn gateway bootstrap thread after {GATEWAY_SPAWN_MAX_ATTEMPTS} attempts: \
                     {last_spawn_err}. The desktop UI will surface this and you can click Restart to retry."
                );
                tracing::error!("[sen-desktop] {msg}");
                {
                    let mut g = server_state.0.lock();
                    g.bootstrap_in_progress = false;
                    g.last_error = Some(msg);
                }
                emit_backend_state(&handle, &server_state);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
    }));

    let app = match app_build {
        Ok(Ok(app)) => app,
        Ok(Err(err)) => {
            tracing::error!(
                "[sen-desktop] tauri builder.build() returned error: {err:#}; \
                 the desktop UI cannot start without the Tauri runtime, exiting after writing diagnostic logs"
            );
            eprintln!(
                "[sen-desktop] failed to build Tauri application: {err}. See {} for details.",
                bootstrap_diag::current_log_dir()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "<no log dir>".to_string()),
            );
            std::process::exit(2);
        }
        Err(payload) => {
            let msg = format_panic_payload(payload);
            tracing::error!(
                "[sen-desktop] tauri builder.build() panicked: {msg}; \
                 the desktop UI cannot start, exiting after writing diagnostic logs"
            );
            eprintln!(
                "[sen-desktop] Tauri builder panicked: {msg}. See {} for details.",
                bootstrap_diag::current_log_dir()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "<no log dir>".to_string()),
            );
            std::process::exit(3);
        }
    };

    app.run(|app_handle, event| match event {
        RunEvent::ExitRequested { .. } => {

            process_lifetime::run_full_shutdown(app_handle, Duration::from_secs(8));
        }
        RunEvent::Exit => {

            process_lifetime::run_full_shutdown(app_handle, Duration::from_secs(2));
        }
        _ => {}
    });
}

fn spawn_gateway_bootstrap_thread(
    handle: AppHandle,
    server_state: ServerState,
) -> Result<(), String> {
    let generation = {
        let mut g = server_state.0.lock();
        if g.bootstrap_in_progress {
            return Err("gateway bootstrap already in progress; skipping duplicate spawn".into());
        }
        g.url = None;
        g.last_error = None;
        g.bootstrap_generation = g.bootstrap_generation.saturating_add(1);
        g.bootstrap_in_progress = true;
        g.last_bootstrap_started_at = Some(Instant::now());
        g.bootstrap_generation
    };
    emit_backend_state(&handle, &server_state);
    let ss = server_state.clone();
    let h = handle.clone();
    let spawn_result = thread::Builder::new()
        .name("sen-gateway-bootstrap".into())
        .spawn(move || {
            run_bootstrap_until_success(ss, generation, h);
        });
    match spawn_result {
        Ok(_) => Ok(()),
        Err(err) => {
            {
                let mut g = server_state.0.lock();
                if g.bootstrap_generation == generation {
                    g.bootstrap_in_progress = false;
                    g.last_error = Some(format!("spawn bootstrap thread: {err}"));
                }
            }
            emit_backend_state(&handle, &server_state);
            Err(format!("spawn gateway bootstrap thread: {err}"))
        }
    }
}

fn run_bootstrap_until_success(server_state: ServerState, generation: u64, handle: AppHandle) {
    let started_at = Instant::now();
    {
        let g = server_state.0.lock();
        if g.bootstrap_generation != generation {
            return;
        }
    }
    match start_embedded_gateway_once(handle.clone(), &server_state, generation) {
        Ok(url) => {
            {
                let mut g = server_state.0.lock();
                if g.bootstrap_generation != generation {
                    return;
                }
                g.url = Some(url);
                g.bootstrap_in_progress = false;
            }
            tracing::info!(
                elapsed_ms = started_at.elapsed().as_millis() as u64,
                "[sen-desktop] embedded gateway is HTTP-ready"
            );
            emit_backend_state(&handle, &server_state);
        }
        Err(err) => {
            let combined = match bootstrap_diag::last_panic_message() {
                Some(panic) => format!("{err}\n--- recent panic captured by panic hook ---\n{panic}"),
                None => err,
            };
            tracing::error!(
                elapsed_ms = started_at.elapsed().as_millis() as u64,
                "[sen-desktop] embedded gateway bootstrap failed; user can request restart via UI: {combined}"
            );
            {
                let mut g = server_state.0.lock();
                if g.bootstrap_generation == generation {
                    g.bootstrap_in_progress = false;
                    g.last_error = Some(combined);
                }
            }
            emit_backend_state(&handle, &server_state);
        }
    }
}

fn reserve_local_port() -> Result<(u16, TcpListener), String> {
    let mut last_err: Option<String> = None;
    for _ in 0..5 {
        match TcpListener::bind("127.0.0.1:0") {
            Ok(listener) => match listener.local_addr() {
                Ok(addr) => {
                    let port = addr.port();

                    return Ok((port, listener));
                }
                Err(err) => last_err = Some(format!("read local port: {err}")),
            },
            Err(err) => last_err = Some(format!("bind local port: {err}")),
        }
    }
    Err(last_err.unwrap_or_else(|| "could not reserve a local port".to_string()))
}

fn probe_health_once(addr: SocketAddr, timeout_ms: u64) -> bool {
    let connect_timeout = Duration::from_millis(timeout_ms.min(5_000));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, connect_timeout) else {
        return false;
    };
    let read_timeout = Duration::from_millis(timeout_ms.max(250));
    let _ = stream.set_read_timeout(Some(read_timeout));
    let _ = stream.set_write_timeout(Some(read_timeout));

    let host = format!("{}:{}", addr.ip(), addr.port());
    let request = format!(
        "GET /health HTTP/1.1\r\n\
         Host: {host}\r\n\
         User-Agent: sen-desktop-bootstrap\r\n\
         {HEALTH_PROBE_HEADER}: {HEALTH_PROBE_HEADER_VALUE}\r\n\
         Connection: close\r\n\
         Accept: */*\r\n\r\n"
    );
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    if stream.flush().is_err() {
        return false;
    }

    let mut buf = [0u8; 256];
    match stream.read(&mut buf) {
        Ok(n) if n >= 12 => {
            let head = std::str::from_utf8(&buf[..n.min(64)]).unwrap_or("");
            head.starts_with("HTTP/1.1 200") || head.starts_with("HTTP/1.0 200")
        }
        _ => false,
    }
}

type GatewayExitChannel = Arc<Mutex<Option<String>>>;

fn record_gateway_exit(channel: &GatewayExitChannel, err: String) {
    let mut slot = channel.lock();
    if slot.is_none() {
        *slot = Some(err);
    }
}

fn record_last_error(server_state: &ServerState, generation: u64, err: &str) {
    let mut g = server_state.0.lock();
    if g.bootstrap_generation == generation {
        g.last_error = Some(err.to_string());
    }
}

fn wait_for_server_until_ready(
    handle: &AppHandle,
    server_state: &ServerState,
    generation: u64,
    host: &str,
    port: u16,
    gateway_exit: &GatewayExitChannel,
) -> Result<(), String> {
    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|err| format!("parse server address: {err}"))?;
    let started = Instant::now();
    let mut last_log = Instant::now();
    let mut diagnostic_pushed = false;
    let hard_deadline = started + Duration::from_secs(GATEWAY_HEALTH_DEADLINE_SECS);
    loop {
        {
            let g = server_state.0.lock();
            if g.bootstrap_generation != generation {
                return Err("bootstrap generation invalidated; abandoning wait".into());
            }
        }
        if let Some(early_err) = gateway_exit.lock().clone() {
            return Err(format!(
                "embedded gateway exited before /health became ready: {early_err}"
            ));
        }
        if probe_health_once(addr, GATEWAY_HEALTH_PROBE_TIMEOUT_MS) {
            tracing::info!(
                elapsed_ms = started.elapsed().as_millis() as u64,
                "[sen-desktop] /health responded OK on {host}:{port}"
            );
            return Ok(());
        }
        let elapsed = started.elapsed();
        if last_log.elapsed() >= Duration::from_secs(10) {
            tracing::info!(
                deadline_secs = GATEWAY_HEALTH_DEADLINE_SECS,
                "[sen-desktop] still waiting for embedded gateway /health on {host}:{port} ({}s elapsed)",
                elapsed.as_secs()
            );
            last_log = Instant::now();
        }
        if !diagnostic_pushed
            && elapsed >= Duration::from_secs(GATEWAY_DIAGNOSTIC_HINT_AFTER_SECS)
        {
            diagnostic_pushed = true;
            let log_dir_hint = sen_log_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| "~/.senweavercoding/logs".to_string());
            let hint = format!(
                "embedded gateway has not responded to /health on {host}:{port} for {}s; \
                 the local server may still be initializing. \
                 If this persists, click \"Open log directory\" and inspect {} \
                 (look for `run_gateway exited`, panic, port-bind error or antivirus interference).",
                elapsed.as_secs(),
                log_dir_hint
            );
            record_last_error(server_state, generation, &hint);
            emit_backend_state(handle, server_state);
        }
        if Instant::now() >= hard_deadline {
            return Err(format!(
                "embedded gateway did not respond to /health on {host}:{port} within {}s; \
                 check {} for backend logs",
                GATEWAY_HEALTH_DEADLINE_SECS,
                sen_log_dir()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "~/.senweavercoding/logs".to_string()),
            ));
        }
        thread::sleep(Duration::from_millis(GATEWAY_HEALTH_PROBE_INTERVAL_MS));
    }
}

fn start_embedded_gateway_once(
    handle: AppHandle,
    server_state: &ServerState,
    generation: u64,
) -> Result<String, String> {
    let host = "127.0.0.1";
    let (port, std_listener) = reserve_local_port()
        .map_err(|err| format!("reserve local port for embedded gateway: {err}"))?;
    std_listener
        .set_nonblocking(true)
        .map_err(|err| format!("listen socket non-blocking: {err}"))?;
    let url = format!("http://{host}:{port}");

    let gateway_exit: GatewayExitChannel = Arc::new(Mutex::new(None));
    let exit_for_thread = Arc::clone(&gateway_exit);

    let spawn_result = thread::Builder::new()
        .name("sen-gateway".into())
        .spawn(move || {
            let exit_for_panic = Arc::clone(&exit_for_thread);
            let work = AssertUnwindSafe(|| {
                let runtime = match tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(err) => {
                        let msg = format!("gateway tokio runtime build failed: {err}");
                        tracing::error!("[sen-desktop] {msg}");
                        record_gateway_exit(&exit_for_thread, msg);
                        return;
                    }
                };
                runtime.block_on(async move {
                    let load_started = Instant::now();
                    let mut config = match senweavercoding::Config::load_or_init().await {
                        Ok(cfg) => cfg,
                        Err(err) => {
                            let msg = format!(
                                "config load failed ({err:#}); embedded gateway will not start to avoid silently dropping API keys / providers. \
                                 Please fix ~/.senweavercoding/config.toml (or the SEN_CONFIG_DIR override) and click Restart in the desktop UI."
                            );
                            tracing::error!("[sen-desktop] {msg}");
                            record_gateway_exit(&exit_for_thread, msg);
                            return;
                        }
                    };
                    tracing::info!(
                        elapsed_ms = load_started.elapsed().as_millis() as u64,
                        "[sen-desktop] gateway: config loaded"
                    );
                    apply_embedded_gateway_overrides(&mut config, host, port);
                    let tokio_listener = match tokio::net::TcpListener::from_std(std_listener) {
                        Ok(l) => l,
                        Err(err) => {
                            let msg = format!(
                                "failed to convert std listener to tokio listener on {host}:{port}: {err}"
                            );
                            tracing::error!("[sen-desktop] {msg}");
                            record_gateway_exit(&exit_for_thread, msg);
                            return;
                        }
                    };
                    let serve_started = Instant::now();
                    if let Err(err) = senweavercoding::gateway::run_gateway_with_supervisors(
                        host,
                        port,
                        config,
                        Some(tokio_listener),
                    )
                    .await
                    {
                        let msg = format!("run_gateway exited: {err:#}");
                        tracing::error!(
                            elapsed_ms = serve_started.elapsed().as_millis() as u64,
                            "[sen-desktop] {msg}"
                        );
                        record_gateway_exit(&exit_for_thread, msg);
                    } else {
                        record_gateway_exit(
                            &exit_for_thread,
                            "run_gateway returned Ok unexpectedly without serving \
                             (this should not happen on a healthy boot)"
                                .to_string(),
                        );
                    }
                });
            });
            if let Err(panic_payload) = std::panic::catch_unwind(work) {
                let msg = format_panic_payload(panic_payload);
                tracing::error!("[sen-desktop] gateway thread panicked: {msg}");
                record_gateway_exit(&exit_for_panic, format!("gateway thread panicked: {msg}"));
            }
        });

    if let Err(err) = spawn_result {
        return Err(format!("spawn gateway thread: {err}"));
    }

    match wait_for_server_until_ready(
        &handle,
        server_state,
        generation,
        host,
        port,
        &gateway_exit,
    ) {
        Ok(()) => Ok(url),
        Err(err) => {
            if let Some(early_err) = gateway_exit.lock().clone() {
                Err(format!("{err}; gateway exit detail: {early_err}"))
            } else {
                Err(err)
            }
        }
    }
}

fn format_panic_payload(payload: Box<dyn std::any::Any + Send + 'static>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        return (*s).to_string();
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    "<non-string panic payload>".to_string()
}

fn apply_embedded_gateway_overrides(
    config: &mut senweavercoding::Config,
    host: &str,
    port: u16,
) {
    if config.gateway.require_pairing {
        tracing::warn!(
            "[sen-desktop] desktop GUI mode is overriding gateway.require_pairing=true to false; \
             the embedded gateway listens on loopback ({host}:{port}) only and shares this process \
             with the webview, so pairing is unnecessary. Run `senweavercoding` headless if you need \
             pairing for remote clients."
        );
    }
    if config.gateway.allow_public_bind {
        tracing::warn!(
            "[sen-desktop] desktop GUI mode is overriding gateway.allow_public_bind=true to false; \
             desktop always binds to 127.0.0.1. Run `senweavercoding` headless to expose the gateway \
             on a public interface."
        );
    }
    if !config.gateway.paired_tokens.is_empty() {
        tracing::warn!(
            paired_tokens = config.gateway.paired_tokens.len(),
            "[sen-desktop] desktop GUI mode is clearing {} paired-token entries from the in-memory \
             config (config.toml on disk is untouched). Headless `senweavercoding` will continue to use them.",
            config.gateway.paired_tokens.len()
        );
    }
    if config.gateway.trust_forwarded_headers {
        tracing::warn!(
            "[sen-desktop] desktop GUI mode is overriding gateway.trust_forwarded_headers=true to false; \
             desktop never sits behind a reverse proxy."
        );
    }
    if config.gateway.path_prefix.is_some() {
        tracing::warn!(
            "[sen-desktop] desktop GUI mode is clearing gateway.path_prefix; webview always talks to \
             the embedded gateway at the root path."
        );
    }
    if config.tunnel.provider.trim() != "none" && !config.tunnel.provider.trim().is_empty() {
        tracing::warn!(
            current = %config.tunnel.provider,
            "[sen-desktop] desktop GUI mode is overriding tunnel.provider to \"none\"; \
             tunneling makes no sense for the loopback-bound embedded gateway."
        );
    }

    config.gateway.host = host.to_string();
    config.gateway.port = port;
    config.gateway.require_pairing = false;
    config.gateway.allow_public_bind = false;
    config.gateway.paired_tokens.clear();
    config.gateway.trust_forwarded_headers = false;
    config.gateway.path_prefix = None;
    config.tunnel.provider = "none".to_string();

    config.browser.enabled = true;
    let backend_norm = config.browser.backend.trim();
    if backend_norm.is_empty() || backend_norm.eq_ignore_ascii_case("agent_browser") {
        config.browser.backend = "auto".to_string();
    }

    let trace_norm = config.observability.runtime_trace_mode.trim();
    if trace_norm.is_empty() || trace_norm.eq_ignore_ascii_case("none") {
        config.observability.runtime_trace_mode = "rolling".to_string();
    }
    if config.observability.runtime_trace_max_entries == 0 {
        config.observability.runtime_trace_max_entries = 500;
    }
}
