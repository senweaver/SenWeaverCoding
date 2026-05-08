

mod browser_dock;
mod terminal;

use std::{
    net::{SocketAddr, TcpListener, TcpStream},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use tauri::{AppHandle, Manager, RunEvent, State};

use browser_dock::DockSharedState;
use terminal::TerminalState;

const EMBEDDED_GATEWAY_PENDING_MSG: &str = "desktop server is starting";

#[cfg(target_os = "windows")]
fn disable_window_focus_border(window: &tauri::WebviewWindow) {
    use std::ptr;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR};
    use windows_sys::Win32::Graphics::Gdi::InvalidateRect;
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, GetWindowLongPtrW, GetWindowRect, IsZoomed, SetWindowLongPtrW,
        SetWindowPos, GWL_STYLE, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCLIENT, HTLEFT, HTRIGHT,
        HTTOP, HTTOPLEFT, HTTOPRIGHT, HWND_TOP, SM_CXPADDEDBORDER, SM_CXSIZEFRAME, SWP_FRAMECHANGED,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WM_ACTIVATE, WM_ACTIVATEAPP,
        WM_NCACTIVATE, WM_NCCALCSIZE, WM_NCHITTEST, WM_NCPAINT, WM_SETFOCUS, WS_BORDER, WS_CAPTION,
        WS_DLGFRAME, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
    };

    const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;
    const SUBCLASS_ID: usize = 0x53_45_4E_57;

    unsafe extern "system" fn no_border_subclass(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _uid: usize,
        _data: usize,
    ) -> LRESULT {
        match msg {

            WM_NCCALCSIZE => 0,
            WM_NCPAINT => 0,

            WM_NCACTIVATE => unsafe { DefSubclassProc(hwnd, msg, 1, lparam) },

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

    let chrome_style_mask: isize = (WS_CAPTION
        | WS_THICKFRAME
        | WS_BORDER
        | WS_DLGFRAME
        | WS_SYSMENU
        | WS_MINIMIZEBOX
        | WS_MAXIMIZEBOX) as isize;
    let prev_style = unsafe { GetWindowLongPtrW(raw, GWL_STYLE) };
    let new_style = prev_style & !chrome_style_mask;
    if new_style != prev_style {
        unsafe {
            SetWindowLongPtrW(raw, GWL_STYLE, new_style);
        }
        tracing::debug!(
            "[sen-desktop] stripped chrome styles: 0x{:08X} -> 0x{:08X}",
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
fn restart_embedded_gateway(handle: AppHandle, state: State<'_, ServerState>) -> Result<(), String> {
    spawn_gateway_bootstrap_thread(handle, state.inner().clone())
}

#[tauri::command]
fn prepare_for_update_install(handle: AppHandle) -> Result<(), String> {
    terminal::shutdown_all(&handle);
    Ok(())
}

pub fn run() {
    let builder = tauri::Builder::default()
        .manage(ServerState::default())
        .manage(TerminalState::default())
        .manage(DockSharedState::new())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            get_server_url,
            restart_embedded_gateway,
            prepare_for_update_install,
            terminal::terminal_spawn,
            terminal::terminal_write,
            terminal::terminal_resize,
            terminal::terminal_kill,
            browser_dock::browser_dock_open,
            browser_dock::browser_dock_set_rect,
            browser_dock::browser_dock_hide,
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
        ]);

    let app = builder
        .setup(|app| {
            #[cfg(target_os = "windows")]
            if let Some(main) = app.get_webview_window("main") {
                disable_window_focus_border(&main);
            }

            browser_dock::install_into(app.handle());

            let handle = app.handle().clone();

            let server_state = app.state::<ServerState>().inner().clone();
            let mut spawn_ok = false;
            let mut last_spawn_err = String::new();
            for attempt in 0usize..128 {
                match spawn_gateway_bootstrap_thread(handle.clone(), server_state.clone()) {
                    Ok(()) => {
                        spawn_ok = true;
                        break;
                    }
                    Err(err) => {
                        last_spawn_err = err.clone();
                        tracing::warn!(
                            "[sen-desktop] gateway bootstrap thread spawn attempt {} failed: {err}",
                            attempt + 1,
                        );
                        thread::sleep(Duration::from_millis(150));
                    }
                }
            }
            if !spawn_ok {
                tracing::error!(
                    "[sen-desktop] giving up spawning gateway bootstrap after 128 attempts: {last_spawn_err}"
                );
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if matches!(event, RunEvent::Exit | RunEvent::ExitRequested { .. }) {
            terminal::shutdown_all(app_handle);
        }
    });
}

fn spawn_gateway_bootstrap_thread(
    handle: AppHandle,
    server_state: ServerState,
) -> Result<(), String> {
    let generation = {
        let mut g = server_state.0.lock();
        g.url = None;
        g.bootstrap_generation = g.bootstrap_generation.saturating_add(1);
        g.bootstrap_generation
    };
    let ss = server_state.clone();
    let h = handle.clone();
    thread::Builder::new()
        .name("sen-gateway-bootstrap".into())
        .spawn(move || {
            run_bootstrap_until_success(ss, generation, h);
        })
        .map_err(|err| format!("spawn gateway bootstrap thread: {err}"))?;
    Ok(())
}

fn run_bootstrap_until_success(server_state: ServerState, generation: u64, handle: AppHandle) {
    let mut attempt: u32 = 0;
    loop {
        match start_embedded_gateway_once(handle.clone()) {
            Ok(url) => {
                let mut g = server_state.0.lock();
                if g.bootstrap_generation != generation {
                    return;
                }
                g.url = Some(url);
                return;
            }
            Err(err) => {
                attempt = attempt.wrapping_add(1);
                tracing::warn!(
                    "[sen-desktop] embedded gateway bootstrap attempt {} failed (will retry inside this generation): {}",
                    attempt,
                    err,
                );
                thread::sleep(Duration::from_secs(1));
                let g = server_state.0.lock();
                if g.bootstrap_generation != generation {
                    return;
                }
                drop(g);
            }
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

fn wait_for_server(host: &str, port: u16) -> Result<(), String> {
    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|err| format!("parse server address: {err}"))?;
    let deadline = Instant::now() + Duration::from_secs(45);
    while Instant::now() < deadline {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(150));
    }
    Err(format!(
        "embedded gateway did not start listening on {host}:{port} within 45 seconds"
    ))
}

fn start_embedded_gateway_once(_handle: AppHandle) -> Result<String, String> {
    let host = "127.0.0.1";
    let (port, std_listener) = reserve_local_port()?;
    std_listener
        .set_nonblocking(true)
        .map_err(|err| format!("listen socket non-blocking: {err}"))?;
    let url = format!("http://{host}:{port}");

    thread::Builder::new()
        .name("sen-gateway".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(err) => {
                    tracing::error!("[sen-desktop] gateway runtime build failed: {err}");
                    return;
                }
            };
            runtime.block_on(async move {
                let mut config = senweavercoding::Config::load_or_init().await.unwrap_or_else(|err| {
                    tracing::warn!(
                        "[sen-desktop] config load failed ({err}); falling back to defaults"
                    );
                    senweavercoding::Config::default()
                });
                apply_embedded_gateway_overrides(&mut config, host, port);
                let tokio_listener = match tokio::net::TcpListener::from_std(std_listener) {
                    Ok(l) => l,
                    Err(err) => {
                        tracing::error!("[sen-desktop] failed to convert std listener to tokio: {err}");
                        return;
                    }
                };
                if let Err(err) =
                    senweavercoding::gateway::run_gateway_with_supervisors(host, port, config, Some(tokio_listener)).await
                {
                    tracing::error!("[sen-desktop] run_gateway exited: {err:#}");
                }
            });
        })
        .map_err(|err| format!("spawn gateway thread: {err}"))?;

    wait_for_server(host, port)?;
    Ok(url)
}

fn apply_embedded_gateway_overrides(
    config: &mut senweavercoding::Config,
    host: &str,
    port: u16,
) {
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
