// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

mod bootstrap_diag;
mod browser_dock;
mod fetch_worker;
mod gateway_bridge;
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
const AGENT_WORKER_STACK_SIZE: usize = 32 * 1024 * 1024;

static GATEWAY_CHILD_PID: Mutex<Option<u32>> = Mutex::new(None);

static QUIT_IN_PROGRESS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn warn_emit_failure(
    counter: &std::sync::atomic::AtomicU64,
    site: &str,
    err: &dyn std::fmt::Display,
) {
    let occurrence = counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    if occurrence == 1 || occurrence % 100 == 0 {
        tracing::warn!("[sen-desktop] {site} emit failed (occurrence {occurrence}): {err}");
    }
}

fn terminate_gateway_pid(pid: u32) {
    tracing::info!("[sen-desktop] terminating isolated gateway child pid={pid}");
    #[cfg(windows)]
    {
        let _ = senweavercoding::util::hidden_sync_command("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status();
    }
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(pid as i32, libc::SIGKILL);
        }
    }
}

pub(crate) fn kill_gateway_child() {
    let pid_opt = GATEWAY_CHILD_PID.lock().take();
    let Some(pid) = pid_opt else { return };
    terminate_gateway_pid(pid);
}

pub(crate) fn kill_gateway_child_pid(pid: u32) {
    {
        let mut slot = GATEWAY_CHILD_PID.lock();
        if *slot == Some(pid) {
            *slot = None;
        } else {
            // The global slot points at a different (newer) child spawned by a
            // concurrent bootstrap; only terminate the pid we own, never the
            // newer one, and leave the slot intact.
            tracing::warn!(
                "[sen-desktop] kill_gateway_child_pid({pid}) but global slot holds {:?}; terminating only our own pid",
                *slot
            );
        }
    }
    terminate_gateway_pid(pid);
}

fn locate_sen_binary() -> Option<PathBuf> {
    let exe_name = if cfg!(windows) { "sen.exe" } else { "sen" };
    if let Ok(current) = std::env::current_exe() {
        if let Some(dir) = current.parent() {
            let candidate = dir.join(exe_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn generate_bridge_token() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = u128::from(std::process::id());
    format!("{:x}{:x}", nanos ^ (pid << 64), pid)
}

fn try_start_isolated_gateway(
    handle: &AppHandle,
    server_state: &ServerState,
    generation: u64,
    host: &str,
    port: u16,
    url: &str,
    gateway_exit: &GatewayExitChannel,
) -> Option<Result<String, String>> {
    let sen_path = match locate_sen_binary() {
        Some(p) => p,
        None => {
            tracing::warn!(
                "[sen-desktop] gateway.isolated=true but the `sen` binary was not found next to \
                 the desktop executable; falling back to the in-process gateway"
            );
            return None;
        }
    };

    let token = generate_bridge_token();
    let log_path = sen_log_dir().map(|d| d.join("gateway-child.log"));
    let mut cmd = senweavercoding::util::hidden_sync_command(sen_path.as_os_str());
    cmd.args(["gateway", "start", "-p", &port.to_string(), "--host", host])
        .env(
            senweavercoding::gateway::desktop_bridge::BRIDGE_MODE_ENV,
            "1",
        )
        .env(
            senweavercoding::gateway::desktop_bridge::BRIDGE_TOKEN_ENV,
            &token,
        );
    if let Some(ref path) = log_path {
        match std::fs::OpenOptions::new().create(true).append(true).open(path) {
            Ok(out_file) => {
                if let Ok(err_file) = out_file.try_clone() {
                    cmd.stdout(out_file).stderr(err_file);
                }
            }
            Err(err) => {
                tracing::warn!(
                    "[sen-desktop] could not open gateway child log file {}: {err}",
                    path.display()
                );
            }
        }
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(err) => {
            tracing::warn!(
                "[sen-desktop] failed to spawn isolated gateway ({err}); falling back to the \
                 in-process gateway"
            );
            return None;
        }
    };
    let pid = child.id();
    *GATEWAY_CHILD_PID.lock() = Some(pid);
    tracing::info!(
        "[sen-desktop] isolated gateway child spawned pid={pid} addr={host}:{port} bin={}",
        sen_path.display()
    );

    let exit_for_watch = Arc::clone(gateway_exit);
    let _ = thread::Builder::new()
        .name("sen-gateway-child-watch".into())
        .spawn(move || {
            let status = child.wait();
            let mut slot = GATEWAY_CHILD_PID.lock();
            if *slot == Some(pid) {
                *slot = None;
            }
            drop(slot);
            record_gateway_exit(
                &exit_for_watch,
                format!("isolated gateway child exited: {status:?}"),
            );
        });

    let result = match wait_for_server_until_ready(
        handle,
        server_state,
        generation,
        host,
        port,
        gateway_exit,
    ) {
        Ok(()) => {
            gateway_bridge::spawn_bridge_client(
                url.to_string(),
                token,
                gateway_bridge::next_bridge_generation(),
            );
            Ok(url.to_string())
        }
        Err(err) => {
            kill_gateway_child_pid(pid);
            if let Some(early_err) = gateway_exit.lock().clone() {
                Err(format!("{err}; gateway exit detail: {early_err}"))
            } else {
                Err(err)
            }
        }
    };
    Some(result)
}

pub(crate) fn current_gateway_url(app: &AppHandle) -> Option<String> {
    app.try_state::<ServerState>()
        .and_then(|s| s.0.lock().url.clone())
}

pub(crate) fn adapters_restart_client() -> &'static reqwest::Client {
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
    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWCP_ROUND: i32 = 2;

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
    let corner_pref: i32 = DWMWCP_ROUND;
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR as u32,
            (&value as *const u32).cast(),
            std::mem::size_of::<u32>() as u32,
        );
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&corner_pref as *const i32).cast(),
            std::mem::size_of::<i32>() as u32,
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
        MONITOR_DEFAULTTONEAREST, RDW_FRAME,
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
        WINDOWPOS, WS_BORDER, WS_CAPTION, WS_DLGFRAME, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU,
        WS_THICKFRAME,
    };

    const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;
    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWCP_ROUND: i32 = 2;
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
                let flags = {
                    let wp = lparam as *const WINDOWPOS;
                    if wp.is_null() {
                        0
                    } else {
                        unsafe { (*wp).flags }
                    }
                };
                let size_changed = (flags & SWP_NOSIZE) == 0;
                unsafe {
                    if size_changed {
                        RedrawWindow(hwnd, ptr::null(), ptr::null_mut(), RDW_FRAME);
                    }
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

    let corner_pref: i32 = DWMWCP_ROUND;
    let corner_hr = unsafe {
        DwmSetWindowAttribute(
            raw,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&corner_pref as *const i32).cast(),
            std::mem::size_of::<i32>() as u32,
        )
    };
    if corner_hr < 0 {
        tracing::debug!(
            "[sen-desktop] DwmSetWindowAttribute(WINDOW_CORNER_PREFERENCE) returned 0x{:08X} \
             (expected on pre-Win11 systems; square corners used as fallback)",
            corner_hr as u32
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

    auto_restart_attempts: u32,
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
async fn restart_embedded_gateway(
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

            guard.bootstrap_in_progress = false;
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
        guard.bootstrap_generation = guard.bootstrap_generation.saturating_add(1);
        guard.auto_restart_attempts = 0;
        guard.url = None;
    }
    emit_backend_state(&handle, state.inner());
    stop_running_gateway_instance().await;
    spawn_gateway_bootstrap_thread(handle, state.inner().clone())
}

async fn stop_running_gateway_instance() {
    let requested = senweavercoding::gateway::request_embedded_shutdown();
    if requested || senweavercoding::gateway::is_running() {
        tracing::info!(
            "[sen-desktop] restart: shutdown requested for the embedded gateway (immediate={requested}); waiting for it to stop"
        );
        let stopped =
            senweavercoding::gateway::wait_embedded_stopped(Duration::from_secs(10)).await;
        if stopped {
            tracing::info!("[sen-desktop] restart: embedded gateway stopped cleanly");
        } else {
            tracing::warn!(
                "[sen-desktop] restart: embedded gateway did not stop within 10s; continuing restart anyway"
            );
        }
    }
    if let Err(err) = tauri::async_runtime::spawn_blocking(kill_gateway_child).await {
        tracing::warn!("[sen-desktop] restart: gateway child kill task failed: {err}");
    }
}

fn stop_running_gateway_instance_blocking() {
    let requested = senweavercoding::gateway::request_embedded_shutdown();
    if requested || senweavercoding::gateway::is_running() {
        tracing::info!(
            "[sen-desktop] auto-restart: shutdown requested for the embedded gateway (immediate={requested}); waiting for it to stop"
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut stopped = false;
        while Instant::now() < deadline {
            if !senweavercoding::gateway::is_running() {
                stopped = true;
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        if stopped {
            tracing::info!("[sen-desktop] auto-restart: embedded gateway stopped cleanly");
        } else {
            tracing::warn!(
                "[sen-desktop] auto-restart: embedded gateway did not stop within 10s; continuing restart anyway"
            );
        }
    }
    kill_gateway_child();
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
        kill_gateway_child();
    })
    .await
    .map_err(|err| format!("update shutdown task failed: {err}"))?;
    Ok(())
}

static FRONTEND_READY_SIGNALED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

const MAIN_WINDOW_BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=document-user-activation-required --disable-background-timer-throttling --disable-renderer-backgrounding --disable-backgrounding-occluded-windows";

static MAIN_WINDOW_REBUILD_FAILURES: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

#[tauri::command]
fn signal_frontend_ready(handle: AppHandle) -> Result<(), String> {
    tracing::info!("[sen-desktop] signal_frontend_ready invoked; revealing main window");
    let first = !FRONTEND_READY_SIGNALED.swap(true, std::sync::atomic::Ordering::SeqCst);
    if first {
        show_and_focus_main_window(&handle);
    }
    Ok(())
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    if QUIT_IN_PROGRESS.swap(true, std::sync::atomic::Ordering::SeqCst) {
        app.exit(0);
        return;
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let shutdown_handle = handle.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            process_lifetime::run_full_shutdown(&shutdown_handle, Duration::from_secs(8));
            kill_gateway_child();
        })
        .await;
        handle.exit(0);
    });
}

#[cfg(target_os = "windows")]
fn force_show_foreground_window(raw: windows_sys::Win32::Foundation::HWND) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, IsIconic, SetForegroundWindow, SetWindowPos, ShowWindow, HWND_NOTOPMOST,
        HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_RESTORE, SW_SHOW,
    };

    if raw.is_null() {
        return;
    }

    unsafe {
        if IsIconic(raw) != 0 {
            ShowWindow(raw, SW_RESTORE);
        } else {
            ShowWindow(raw, SW_SHOW);
        }

        SetWindowPos(
            raw,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
        SetWindowPos(
            raw,
            HWND_NOTOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
        BringWindowToTop(raw);
        SetForegroundWindow(raw);
    }

    reapply_chrome_styles(raw);
}

fn hide_minimal_window(app: &AppHandle) {
    if let Some(minimal) = app.get_webview_window("minimal") {
        if minimal.is_visible().unwrap_or(false) {
            let _ = minimal.hide();
        }
    }
    if let Some(input) = app.get_webview_window("minimal-input") {
        if input.is_visible().unwrap_or(false) {
            let _ = input.hide();
        }
    }
}

// Rebuilds the "main" window from scratch, mirroring the config in
// tauri.conf.json. This is the recovery path for the case where the config
// window failed to register (or was destroyed) and is therefore absent from
// the runtime window map. MUST be called on the main thread.
fn build_main_window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};

    let builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("SenWeaverCoding")
        .inner_size(1280.0, 800.0)
        .min_inner_size(480.0, 360.0)
        .center()
        .resizable(true)
        .decorations(false)
        .shadow(false)
        .transparent(true)
        .accept_first_mouse(true)
        .visible(false)
        .additional_browser_args(MAIN_WINDOW_BROWSER_ARGS);

    match builder.build() {
        Ok(win) => {
            tracing::warn!(
                "[sen-desktop] reveal main: 'main' window was absent from the window map; rebuilt it from scratch"
            );
            if let Err(err) = win.set_resizable(true) {
                tracing::debug!("[sen-desktop] rebuilt main set_resizable failed: {err}");
            }
            #[cfg(target_os = "windows")]
            disable_window_focus_border(&win);
            schedule_frontend_ready_watchdog(win.clone());
            Some(win)
        }
        Err(err) => {
            warn_emit_failure(
                &MAIN_WINDOW_REBUILD_FAILURES,
                "rebuild main window",
                &err,
            );
            None
        }
    }
}

// Returns the live "main" window, recreating it only if it is truly gone.
//
// Crucially this uses the *window* registry (`get_window`) rather than
// `get_webview_window`: once the embedded-browser dock attaches a child webview
// to the main window (`browser_dock::add_child`), the window is no longer a
// simple 1:1 `WebviewWindow`, so `get_webview_window("main")` returns `None`
// even though the window is alive and well. `get_window("main")` keeps working,
// and every method used for revealing (`show`/`set_focus`/`unminimize`/`hwnd`)
// is available on `Window`.
//
// MUST be called on the main thread because window creation is main-thread-only.
fn ensure_main_window(app: &AppHandle) -> Option<tauri::Window> {
    if let Some(win) = app.get_window("main") {
        return Some(win);
    }
    build_main_window(app);
    app.get_window("main")
}

fn show_and_focus_main_window(app: &AppHandle) {
    let app = app.clone();
    let dispatched = app.clone().run_on_main_thread(move || {
        let webview_labels: Vec<String> = app.webview_windows().keys().cloned().collect();
        let window_labels: Vec<String> = app.windows().keys().cloned().collect();
        let Some(win) = ensure_main_window(&app) else {
            tracing::error!(
                "[sen-desktop] reveal main: could not obtain or rebuild the 'main' window (windows={window_labels:?} webviews={webview_labels:?})"
            );
            return;
        };
        tracing::info!(
            "[sen-desktop] reveal main: target={:?} windows={:?} webviews={:?}",
            win.label(),
            window_labels,
            webview_labels
        );

        if win.is_minimized().unwrap_or(false) {
            let _ = win.unminimize();
        }
        if let Err(err) = win.show() {
            tracing::warn!("[sen-desktop] reveal main show() failed: {err}");
        }
        let _ = win.set_focus();
        #[cfg(target_os = "windows")]
        if let Ok(hwnd) = win.hwnd() {
            use windows_sys::Win32::Foundation::HWND;
            let raw = hwnd.0 as HWND;
            force_show_foreground_window(raw);
            reapply_chrome_styles(raw);
        }
        hide_minimal_window(&app);
        tracing::info!(
            "[sen-desktop] reveal main (main-thread): label={:?} visible={:?} minimized={:?} pos={:?} size={:?} scale={:?}",
            win.label(),
            win.is_visible(),
            win.is_minimized(),
            win.outer_position().ok(),
            win.outer_size().ok(),
            win.scale_factor().ok(),
        );
    });
    if let Err(err) = dispatched {
        tracing::warn!("[sen-desktop] reveal main run_on_main_thread failed: {err}");
    }
}

const TRAY_QUIT_EVENT: &str = "tray://quit-requested";
const TRAY_COMPUTER_STOP_EVENT: &str = "minimal://computer-stop";

struct TrayMenuItems {
    show: tauri::menu::MenuItem<tauri::Wry>,
    stop_computer: tauri::menu::MenuItem<tauri::Wry>,
    quit: tauri::menu::MenuItem<tauri::Wry>,
}

#[tauri::command]
fn set_tray_labels(
    state: tauri::State<'_, TrayMenuItems>,
    show: String,
    stop_computer: String,
    quit: String,
) -> Result<(), String> {
    if !show.trim().is_empty() {
        state.show.set_text(show).map_err(|e| e.to_string())?;
    }
    if !stop_computer.trim().is_empty() {
        state
            .stop_computer
            .set_text(stop_computer)
            .map_err(|e| e.to_string())?;
    }
    if !quit.trim().is_empty() {
        state.quit.set_text(quit).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn setup_system_tray(app: &AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    // Initial labels are English (the app's primary locale); the front-end pushes
    // localized labels via `set_tray_labels` on boot and whenever the UI locale
    // changes, so the tray stays in sync with the in-app language.
    let show_item = MenuItem::with_id(app, "tray_show", "Show main window", true, None::<&str>)?;
    let stop_computer_item =
        MenuItem::with_id(app, "tray_stop_computer", "Stop computer control", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "tray_quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &stop_computer_item, &quit_item])?;
    app.manage(TrayMenuItems {
        show: show_item.clone(),
        stop_computer: stop_computer_item.clone(),
        quit: quit_item.clone(),
    });

    let mut builder = TrayIconBuilder::with_id("sen-main-tray")
        .tooltip("SenWeaverCoding")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "tray_show" => show_and_focus_main_window(app),
            "tray_stop_computer" => {
                if let Err(err) = app.emit(TRAY_COMPUTER_STOP_EVENT, ()) {
                    tracing::warn!("[sen-desktop] emit {TRAY_COMPUTER_STOP_EVENT} failed: {err}");
                }
            }
            "tray_quit" => {
                if let Err(err) = app.emit(TRAY_QUIT_EVENT, ()) {
                    tracing::warn!("[sen-desktop] emit {TRAY_QUIT_EVENT} failed: {err}");
                }
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
            | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => {
                show_and_focus_main_window(tray.app_handle());
            }
            _ => {}
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;
    Ok(())
}

const FRONTEND_READY_TIMEOUT_MS: u64 = 60_000;

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
        force_show_foreground_window(raw);
        reapply_chrome_styles(raw);
    }

    tracing::info!(
        "[sen-desktop] show_main_window_now: visible={:?}",
        window.is_visible()
    );
}

fn schedule_frontend_ready_watchdog(window: tauri::WebviewWindow) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(FRONTEND_READY_TIMEOUT_MS));
        if FRONTEND_READY_SIGNALED.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let win = window.clone();
        if let Err(err) = window.run_on_main_thread(move || {
            // The window may have been revealed through any path (including the
            // frontend's direct show() fallback) without the explicit signal.
            // If it is already visible the app started fine; never show the
            // failure screen in that case.
            if win.is_visible().unwrap_or(false) {
                return;
            }
            tracing::error!(
                "[sen-desktop] frontend did not signal ready within {}s and window is not visible; showing boot-failure screen",
                FRONTEND_READY_TIMEOUT_MS / 1_000
            );
            show_frontend_failure(&win);
        }) {
            tracing::error!("[sen-desktop] watchdog: run_on_main_thread failed: {err}");
        }
    });
}

fn show_frontend_failure(window: &tauri::WebviewWindow) {
    let secs = FRONTEND_READY_TIMEOUT_MS / 1_000;
    if let Err(err) = window.eval(&frontend_failure_script(secs)) {
        tracing::warn!("[sen-desktop] failed to inject boot-failure screen: {err}");
    }
    show_main_window_now(window);
}

fn frontend_failure_script(secs: u64) -> String {
    let default_reason = format!(
        "前端在 {secs} 秒内未发出就绪信号，可能是脚本未能加载或初始化时卡死。\\nThe frontend did not become ready within {secs}s; the script may have failed to load or hung during initialization."
    );
    format!(
        r#"(function(){{
  try {{
    var root = document.getElementById('root');
    if (!root) return;
    if (root.dataset && root.dataset.bootErrorPainted === '1') return;
    var reason = '';
    try {{ reason = (window.__SEN_BOOT_ERROR__ || '').toString(); }} catch (e) {{ reason = ''; }}
    if (!reason) reason = "{default_reason}";
    root.innerHTML = '';
    root.dataset.bootErrorPainted = '1';
    var dark = false;
    try {{ dark = window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches; }} catch (e) {{}}
    var fg = dark ? '#ececec' : '#1a1a1a';
    var sub = dark ? '#aaaaaa' : '#444444';
    var bg = dark ? '#0E0E0E' : '#FCFCFC';
    var pre_bg = dark ? '#1a1a1a' : '#f4f4f4';
    var pre_bd = dark ? '#333333' : '#dddddd';
    var wrap = document.createElement('div');
    wrap.style.cssText = 'min-height:100vh;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:12px;padding:24px;font:13px -apple-system,BlinkMacSystemFont,Segoe UI,Roboto,Arial,sans-serif;color:'+fg+';background:'+bg+';';
    var title = document.createElement('div');
    title.textContent = 'React 应用启动失败 / Frontend failed to start';
    title.style.cssText = 'font-size:15px;font-weight:600;';
    var body = document.createElement('pre');
    body.textContent = reason;
    body.style.cssText = 'font-size:11px;max-height:300px;max-width:820px;width:100%;overflow:auto;background:'+pre_bg+';border:1px solid '+pre_bd+';border-radius:6px;padding:12px;white-space:pre-wrap;word-break:break-all;color:'+sub+';';
    var btn = document.createElement('button');
    btn.textContent = '重新加载 / Reload';
    btn.style.cssText = 'padding:6px 14px;font-size:13px;border:1px solid #888;border-radius:6px;background:transparent;color:'+fg+';cursor:pointer;';
    btn.onclick = function() {{ window.location.reload(); }};
    wrap.appendChild(title);
    wrap.appendChild(body);
    wrap.appendChild(btn);
    root.appendChild(wrap);
  }} catch (e) {{}}
}})();"#
    )
}

#[derive(serde::Deserialize)]
struct CuratorDiagramInput {
    code: String,
    png_base64: String,
}

fn strip_extended_length_prefix(raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    if let Some(rest) = raw.strip_prefix(r"\\?\") {
        return rest.to_string();
    }
    raw.to_string()
}

#[tauri::command]
async fn curator_render_docx_with_diagrams(
    final_md_path: String,
    template: String,
    diagrams: Vec<CuratorDiagramInput>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        curator_render_docx_with_diagrams_blocking(final_md_path, template, diagrams)
    })
    .await
    .map_err(|err| format!("docx render task failed: {err}"))?
}

fn curator_render_docx_with_diagrams_blocking(
    final_md_path: String,
    template: String,
    diagrams: Vec<CuratorDiagramInput>,
) -> Result<String, String> {
    use base64::Engine as _;
    use senweavercoding::tools::curator::docx::render_docx_with_diagrams;
    use senweavercoding::tools::curator::CuratorTemplateKind;

    let md_path = PathBuf::from(&final_md_path);
    let markdown = std::fs::read_to_string(&md_path)
        .map_err(|e| format!("failed to read final.md at {}: {e}", md_path.display()))?;
    let parent = md_path
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| "final.md has no parent directory".to_string())?;

    let assets_dir = parent.join(".curator-assets");
    std::fs::create_dir_all(&assets_dir)
        .map_err(|e| format!("failed to create assets dir: {e}"))?;

    let mut decoded: Vec<(String, PathBuf)> = Vec::with_capacity(diagrams.len());
    for (idx, diagram) in diagrams.iter().enumerate() {
        let raw = diagram
            .png_base64
            .split_once("base64,")
            .map(|(_, rest)| rest)
            .unwrap_or(diagram.png_base64.as_str())
            .trim();
        if raw.is_empty() {
            continue;
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(raw)
            .map_err(|e| format!("failed to decode diagram {idx} png: {e}"))?;
        let png_path = assets_dir.join(format!("mermaid-{idx}.png"));
        std::fs::write(&png_path, &bytes)
            .map_err(|e| format!("failed to write diagram {idx} png: {e}"))?;
        decoded.push((diagram.code.clone(), png_path));
    }

    if decoded.is_empty() {
        return Err("no usable diagrams were provided".to_string());
    }

    let docx_path = parent.join("final.docx");
    let kind = CuratorTemplateKind::from_str_loose(&template);
    render_docx_with_diagrams(&markdown, kind, &docx_path, &decoded)
        .map_err(|e| format!("docx render failed: {e}"))?;

    Ok(strip_extended_length_prefix(&docx_path.to_string_lossy()))
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

#[derive(serde::Serialize)]
struct LocalImageData {
    mime: String,
    #[serde(rename = "dataUrl")]
    data_url: String,
}

const MAX_LOCAL_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

fn image_mime_from_ext(path: &std::path::Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" | "jfif" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        "heic" | "heif" => "image/heic",
        "tif" | "tiff" => "image/tiff",
        _ => return None,
    })
}

#[tauri::command]
async fn read_local_image_data_url(path: String) -> Result<LocalImageData, String> {
    tauri::async_runtime::spawn_blocking(move || read_local_image_data_url_blocking(path))
        .await
        .map_err(|err| format!("read image task failed: {err}"))?
}

fn read_local_image_data_url_blocking(path: String) -> Result<LocalImageData, String> {
    use base64::Engine as _;

    let p = PathBuf::from(&path);
    let meta = std::fs::metadata(&p).map_err(|e| format!("cannot stat {path}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("not a file: {path}"));
    }
    if meta.len() > MAX_LOCAL_IMAGE_BYTES {
        return Err(format!(
            "image too large ({} bytes, max {MAX_LOCAL_IMAGE_BYTES})",
            meta.len()
        ));
    }
    let mime =
        image_mime_from_ext(&p).ok_or_else(|| format!("unsupported image type: {path}"))?;
    let bytes = std::fs::read(&p).map_err(|e| format!("cannot read {path}: {e}"))?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(LocalImageData {
        mime: mime.to_string(),
        data_url: format!("data:{mime};base64,{b64}"),
    })
}

const MINIMAL_INPUT_HIDDEN_EVENT: &str = "minimal://input-hidden";

#[cfg(target_os = "windows")]
static MINIMAL_INPUT_WATCHING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn hide_minimal_input_and_notify(app: &AppHandle) {
    if let Some(input) = app.get_webview_window("minimal-input") {
        if input.is_visible().unwrap_or(false) {
            let _ = input.hide();
        }
    }
    let _ = app.emit(MINIMAL_INPUT_HIDDEN_EVENT, ());
}

#[cfg(target_os = "windows")]
fn spawn_minimal_input_foreground_watch(app: &AppHandle) {
    use std::sync::atomic::Ordering;

    if MINIMAL_INPUT_WATCHING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetAncestor, GetForegroundWindow, GA_ROOT,
        };

        std::thread::sleep(std::time::Duration::from_millis(300));
        loop {
            std::thread::sleep(std::time::Duration::from_millis(120));
            let Some(input) = app.get_webview_window("minimal-input") else {
                break;
            };
            if !input.is_visible().unwrap_or(false) {
                break;
            }
            let input_hwnd = match input.hwnd() {
                Ok(h) => h.0 as HWND,
                Err(_) => break,
            };
            let card_hwnd = app
                .get_webview_window("minimal")
                .and_then(|w| w.hwnd().ok())
                .map(|h| h.0 as HWND)
                .unwrap_or(std::ptr::null_mut());
            let fg_root = unsafe {
                let fg = GetForegroundWindow();
                if fg.is_null() {
                    std::ptr::null_mut()
                } else {
                    GetAncestor(fg, GA_ROOT)
                }
            };
            let inside = !fg_root.is_null()
                && (fg_root == input_hwnd || (!card_hwnd.is_null() && fg_root == card_hwnd));
            if !inside {
                hide_minimal_input_and_notify(&app);
                break;
            }
        }
        MINIMAL_INPUT_WATCHING.store(false, Ordering::SeqCst);
    });
}

#[cfg(target_os = "windows")]
fn force_activate_window(hwnd: windows_sys::Win32::Foundation::HWND) {
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
    };

    if hwnd.is_null() {
        return;
    }
    unsafe {
        let fg = GetForegroundWindow();
        let this_thread = GetCurrentThreadId();
        let mut fg_thread = 0u32;
        let mut attached = false;
        if !fg.is_null() && fg != hwnd {
            fg_thread = GetWindowThreadProcessId(fg, std::ptr::null_mut());
            if fg_thread != 0 && fg_thread != this_thread {
                attached = AttachThreadInput(fg_thread, this_thread, 1) != 0;
            }
        }
        BringWindowToTop(hwnd);
        SetForegroundWindow(hwnd);
        SetFocus(hwnd);
        if attached {
            AttachThreadInput(fg_thread, this_thread, 0);
        }
    }
}

#[cfg(target_os = "windows")]
fn disable_show_transitions(hwnd: windows_sys::Win32::Foundation::HWND) {
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;

    const DWMWA_TRANSITIONS_FORCEDISABLED: u32 = 3;
    if hwnd.is_null() {
        return;
    }
    let value: i32 = 1;
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_TRANSITIONS_FORCEDISABLED,
            (&value as *const i32).cast(),
            std::mem::size_of::<i32>() as u32,
        );
    }
}

#[tauri::command]
fn minimal_input_show(app: AppHandle, width: f64, height: f64) -> Result<(), String> {
    let card = app
        .get_webview_window("minimal")
        .ok_or_else(|| "minimal window missing".to_string())?;
    let input = app
        .get_webview_window("minimal-input")
        .ok_or_else(|| "minimal-input window missing".to_string())?;

    if input.is_visible().unwrap_or(false) {
        hide_minimal_input_and_notify(&app);
        return Ok(());
    }

    let scale = card.scale_factor().map_err(|e| e.to_string())?;
    let card_pos = card.outer_position().map_err(|e| e.to_string())?;
    let card_size = card.outer_size().map_err(|e| e.to_string())?;

    let new_w = ((width * scale).round() as i32).max(1);
    let new_h = ((height * scale).round() as i32).max(1);
    let overlap = (14.0 * scale).round() as i32;
    let x = card_pos.x + card_size.width as i32 - new_w;
    let y = card_pos.y + overlap - new_h;

    input
        .set_size(tauri::PhysicalSize::new(new_w as u32, new_h as u32))
        .map_err(|e| e.to_string())?;
    input
        .set_position(tauri::PhysicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;

    #[cfg(target_os = "windows")]
    if let Ok(hwnd) = input.hwnd() {
        use windows_sys::Win32::Foundation::HWND;
        let raw = hwnd.0 as HWND;
        disable_show_transitions(raw);
        reapply_chrome_styles(raw);
    }

    input.show().map_err(|e| e.to_string())?;

    let input_focus = input.clone();
    let dispatched = app.run_on_main_thread(move || {
        let _ = input_focus.set_focus();
        #[cfg(target_os = "windows")]
        if let Ok(hwnd) = input_focus.hwnd() {
            force_activate_window(hwnd.0 as windows_sys::Win32::Foundation::HWND);
        }
    });
    if dispatched.is_err() {
        let _ = input.set_focus();
    }

    #[cfg(target_os = "windows")]
    spawn_minimal_input_foreground_watch(&app);
    Ok(())
}

#[tauri::command]
fn minimal_input_hide(app: AppHandle) -> Result<(), String> {
    hide_minimal_input_and_notify(&app);
    Ok(())
}

#[tauri::command]
fn minimal_resize_anchored(
    window: tauri::WebviewWindow,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let new_w = (width * scale).round() as i32;
    let new_h = (height * scale).round() as i32;
    if new_w <= 0 || new_h <= 0 {
        return Err(format!("invalid target size: {width}x{height}"));
    }

    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::{HWND, RECT};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowRect, SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_NOZORDER,
        };

        let hwnd = window.hwnd().map_err(|e| e.to_string())?;
        let raw = hwnd.0 as HWND;
        if raw.is_null() {
            return Err("window handle unavailable".to_string());
        }
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        unsafe {
            if GetWindowRect(raw, &mut rect) == 0 {
                return Err("GetWindowRect failed".to_string());
            }
            SetWindowPos(
                raw,
                HWND_TOP,
                rect.right - new_w,
                rect.bottom - new_h,
                new_w,
                new_h,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    {
        let pos = window.outer_position().map_err(|e| e.to_string())?;
        let size = window.outer_size().map_err(|e| e.to_string())?;
        let x = pos.x + size.width as i32 - new_w;
        let y = pos.y + size.height as i32 - new_h;
        window
            .set_size(tauri::PhysicalSize::new(new_w as u32, new_h as u32))
            .map_err(|e| e.to_string())?;
        window
            .set_position(tauri::PhysicalPosition::new(x, y))
            .map_err(|e| e.to_string())?;
        Ok(())
    }
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
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_and_focus_main_window(app);
        }))
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
            quit_app,
            set_tray_labels,
            reveal_in_explorer,
            read_local_image_data_url,
            minimal_resize_anchored,
            minimal_input_show,
            minimal_input_hide,
            curator_render_docx_with_diagrams,
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
            browser_dock::browser_dock_present_session,
            browser_dock::browser_dock_set_foreground_session,
        ]);

    let app_build = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        builder
        .setup(|app| {
            let initial_labels: Vec<String> =
                app.webview_windows().keys().cloned().collect();
            tracing::info!(
                "[sen-desktop] setup: config-created windows = {initial_labels:?}"
            );

            if let Some(main) = app.get_webview_window("main") {
                if let Err(err) = main.set_resizable(true) {
                    tracing::debug!("[sen-desktop] set_resizable(true) failed: {err}");
                }

                #[cfg(target_os = "windows")]
                disable_window_focus_border(&main);

                schedule_frontend_ready_watchdog(main.clone());
            } else {
                tracing::error!(
                    "[sen-desktop] setup: the 'main' window is MISSING from the config-created \
                     window map (labels={initial_labels:?}); it will be rebuilt on first reveal"
                );
            }

            if let Some(minimal) = app.get_webview_window("minimal") {
                let _ = minimal.hide();
                #[cfg(target_os = "windows")]
                disable_window_focus_border(&minimal);
            }

            if let Some(minimal_input) = app.get_webview_window("minimal-input") {
                let _ = minimal_input.hide();
                #[cfg(target_os = "windows")]
                disable_window_focus_border(&minimal_input);
                #[cfg(target_os = "windows")]
                if let Ok(hwnd) = minimal_input.hwnd() {
                    disable_show_transitions(hwnd.0 as windows_sys::Win32::Foundation::HWND);
                }
            }

            browser_dock::install_into(app.handle());
            fetch_worker::install_into(app.handle());

            if let Err(err) = setup_system_tray(app.handle()) {
                tracing::warn!("[sen-desktop] system tray setup failed: {err}");
            }

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
            let _ = app_handle.remove_tray_by_id("sen-main-tray");
            process_lifetime::run_full_shutdown(app_handle, Duration::from_secs(8));
            kill_gateway_child();
        }
        RunEvent::Exit => {
            let _ = app_handle.remove_tray_by_id("sen-main-tray");
            process_lifetime::run_full_shutdown(app_handle, Duration::from_secs(2));
            kill_gateway_child();
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
        Ok(launch) => {
            {
                let mut g = server_state.0.lock();
                if g.bootstrap_generation != generation {
                    return;
                }
                g.url = Some(launch.url.clone());
                g.bootstrap_in_progress = false;
            }
            tracing::info!(
                elapsed_ms = started_at.elapsed().as_millis() as u64,
                "[sen-desktop] embedded gateway is HTTP-ready"
            );
            emit_backend_state(&handle, &server_state);
            spawn_gateway_crash_watcher(
                handle,
                server_state,
                generation,
                launch.exit_channel,
                launch.in_process,
            );
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

struct GatewayLaunch {
    url: String,
    exit_channel: GatewayExitChannel,
    in_process: bool,
}

fn record_gateway_exit(channel: &GatewayExitChannel, err: String) {
    let mut slot = channel.lock();
    if slot.is_none() {
        *slot = Some(err);
    }
}

const GATEWAY_AUTO_RESTART_MAX_ATTEMPTS: u32 = 3;
const GATEWAY_STABLE_UPTIME_RESET: Duration = Duration::from_secs(600);
const GATEWAY_CRASH_POLL_INTERVAL_MS: u64 = 500;

fn spawn_gateway_crash_watcher(
    handle: AppHandle,
    server_state: ServerState,
    generation: u64,
    exit_channel: GatewayExitChannel,
    in_process: bool,
) {
    let spawn_result = thread::Builder::new()
        .name("sen-gateway-watch".into())
        .spawn(move || {
            let ready_at = Instant::now();
            loop {
                thread::sleep(Duration::from_millis(GATEWAY_CRASH_POLL_INTERVAL_MS));
                if process_lifetime::is_shutting_down() {
                    return;
                }
                {
                    let g = server_state.0.lock();
                    if g.bootstrap_generation != generation {
                        return;
                    }
                }
                let mut exit_reason = exit_channel.lock().clone();
                if exit_reason.is_none()
                    && in_process
                    && senweavercoding::gateway::is_fully_stopped()
                {
                    exit_reason =
                        Some("embedded gateway stopped without an exit report".to_string());
                }
                let Some(reason) = exit_reason else {
                    continue;
                };
                handle_gateway_exit_after_ready(
                    handle,
                    server_state,
                    generation,
                    reason,
                    ready_at.elapsed(),
                );
                return;
            }
        });
    if let Err(err) = spawn_result {
        tracing::warn!("[sen-desktop] could not spawn gateway crash watcher: {err}");
    }
}

fn handle_gateway_exit_after_ready(
    handle: AppHandle,
    server_state: ServerState,
    generation: u64,
    reason: String,
    uptime: Duration,
) {
    tracing::error!(
        uptime_ms = uptime.as_millis() as u64,
        "[sen-desktop] gateway exited after becoming ready: {reason}"
    );
    let attempt = {
        let mut g = server_state.0.lock();
        if g.bootstrap_generation != generation {
            return;
        }
        if uptime >= GATEWAY_STABLE_UPTIME_RESET {
            g.auto_restart_attempts = 0;
        }
        g.url = None;
        g.bootstrap_in_progress = false;
        g.last_error = Some(format!("gateway exited unexpectedly: {reason}"));
        g.auto_restart_attempts = g.auto_restart_attempts.saturating_add(1);
        g.auto_restart_attempts
    };
    emit_backend_state(&handle, &server_state);
    if attempt > GATEWAY_AUTO_RESTART_MAX_ATTEMPTS {
        tracing::error!(
            "[sen-desktop] gateway crashed more than {GATEWAY_AUTO_RESTART_MAX_ATTEMPTS} times; \
             staying in the failed state until the user restarts manually"
        );
        return;
    }
    let delay = match attempt {
        1 => Duration::from_secs(2),
        2 => Duration::from_secs(5),
        _ => Duration::from_secs(15),
    };
    tracing::warn!(
        "[sen-desktop] auto-restarting gateway (attempt {attempt}/{GATEWAY_AUTO_RESTART_MAX_ATTEMPTS}) in {}s",
        delay.as_secs()
    );
    thread::sleep(delay);
    if process_lifetime::is_shutting_down() {
        return;
    }
    {
        let g = server_state.0.lock();
        if g.bootstrap_generation != generation {
            return;
        }
    }
    stop_running_gateway_instance_blocking();
    if let Err(err) = spawn_gateway_bootstrap_thread(handle, server_state) {
        tracing::error!("[sen-desktop] gateway auto-restart could not spawn bootstrap: {err}");
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

fn is_port_bind_error(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    lower.contains("bind")
        || lower.contains("address already in use")
        || lower.contains("addrinuse")
        || lower.contains("10048")
        || lower.contains("os error 98")
}

fn start_isolated_gateway_with_port_retry(
    handle: &AppHandle,
    server_state: &ServerState,
    generation: u64,
    host: &'static str,
    first_port: u16,
) -> Result<Option<GatewayLaunch>, String> {
    const ISOLATED_PORT_ATTEMPTS: usize = 3;
    let mut port = first_port;
    for attempt in 0..ISOLATED_PORT_ATTEMPTS {
        let url = format!("http://{host}:{port}");
        let gateway_exit: GatewayExitChannel = Arc::new(Mutex::new(None));
        match try_start_isolated_gateway(
            handle,
            server_state,
            generation,
            host,
            port,
            &url,
            &gateway_exit,
        ) {
            Some(Ok(ready_url)) => {
                return Ok(Some(GatewayLaunch {
                    url: ready_url,
                    exit_channel: gateway_exit,
                    in_process: false,
                }));
            }
            Some(Err(err)) => {
                if attempt + 1 < ISOLATED_PORT_ATTEMPTS && is_port_bind_error(&err) {
                    tracing::warn!(
                        "[sen-desktop] isolated gateway attempt {} failed with a bind-class error on {host}:{port} ({err}); retrying on a fresh port",
                        attempt + 1
                    );
                    let (next_port, next_listener) = reserve_local_port().map_err(|e| {
                        format!("re-reserve local port for isolated gateway retry: {e}")
                    })?;
                    drop(next_listener);
                    port = next_port;
                    continue;
                }
                return Err(err);
            }
            None => return Ok(None),
        }
    }
    Err("isolated gateway could not bind a local port after retries".to_string())
}

fn start_embedded_gateway_once(
    handle: AppHandle,
    server_state: &ServerState,
    generation: u64,
) -> Result<GatewayLaunch, String> {
    let host = "127.0.0.1";
    let (port, std_listener) = reserve_local_port()
        .map_err(|err| format!("reserve local port for embedded gateway: {err}"))?;
    std_listener
        .set_nonblocking(true)
        .map_err(|err| format!("listen socket non-blocking: {err}"))?;

    kill_gateway_child();
    let _ = gateway_bridge::next_bridge_generation();

    if senweavercoding::config::sniff_gateway_isolated() {
        drop(std_listener);
        match start_isolated_gateway_with_port_retry(
            &handle,
            server_state,
            generation,
            host,
            port,
        )? {
            Some(launch) => return Ok(launch),
            None => {
                let (port_retry, listener_retry) = reserve_local_port()
                    .map_err(|err| format!("re-reserve local port for embedded gateway: {err}"))?;
                listener_retry
                    .set_nonblocking(true)
                    .map_err(|err| format!("listen socket non-blocking: {err}"))?;
                return start_in_process_gateway(
                    handle,
                    server_state,
                    generation,
                    host,
                    port_retry,
                    listener_retry,
                );
            }
        }
    }

    start_in_process_gateway(handle, server_state, generation, host, port, std_listener)
}

fn start_in_process_gateway(
    handle: AppHandle,
    server_state: &ServerState,
    generation: u64,
    host: &'static str,
    port: u16,
    std_listener: TcpListener,
) -> Result<GatewayLaunch, String> {
    let gateway_exit: GatewayExitChannel = Arc::new(Mutex::new(None));
    let exit_for_thread = Arc::clone(&gateway_exit);
    let url = format!("http://{host}:{port}");
    let spawn_result = thread::Builder::new()
        .name("sen-gateway".into())
        .spawn(move || {
            let exit_for_panic = Arc::clone(&exit_for_thread);
            let work = AssertUnwindSafe(|| {
                let gateway_workers = std::thread::available_parallelism()
                    .map(|n| n.get().saturating_sub(2).max(2))
                    .unwrap_or(2);
                let runtime = match tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .worker_threads(gateway_workers)
                    .thread_stack_size(AGENT_WORKER_STACK_SIZE)
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
        Ok(()) => Ok(GatewayLaunch {
            url,
            exit_channel: gateway_exit,
            in_process: true,
        }),
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
