

mod browser_dock;
mod fetch_worker;
mod process_lifetime;
mod terminal;

use std::{
    io::{Read, Write},
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
const GATEWAY_HEALTH_DEADLINE_SECS: u64 = 600;
const GATEWAY_HEALTH_PROBE_INTERVAL_MS: u64 = 250;
const GATEWAY_HEALTH_PROBE_TIMEOUT_MS: u64 = 2_000;
const RESTART_DEBOUNCE_SECS: u64 = 90;

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

    let chrome_style_mask: isize =
        (WS_CAPTION | WS_THICKFRAME | WS_BORDER | WS_DLGFRAME) as isize;
    let required_style_bits: isize = (WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX) as isize;
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
        InvalidateRect, RedrawWindow, RDW_FRAME, RDW_INVALIDATE,
    };
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, GetWindowLongPtrW, GetWindowRect, IsZoomed, SetWindowLongPtrW,
        SetWindowPos, GWL_STYLE, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCLIENT, HTLEFT, HTRIGHT,
        HTTOP, HTTOPLEFT, HTTOPRIGHT, HWND_TOP, SM_CXPADDEDBORDER, SM_CXSIZEFRAME, SWP_FRAMECHANGED,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WM_ACTIVATE, WM_ACTIVATEAPP,
        WM_DPICHANGED, WM_DWMCOMPOSITIONCHANGED, WM_DWMNCRENDERINGCHANGED, WM_NCACTIVATE,
        WM_NCCALCSIZE, WM_NCHITTEST, WM_NCPAINT, WM_SETFOCUS, WM_SETTINGCHANGE, WM_SHOWWINDOW,
        WM_THEMECHANGED, WM_WINDOWPOSCHANGED, WS_BORDER, WS_CAPTION, WS_DLGFRAME, WS_MAXIMIZEBOX,
        WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
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
            WM_NCCALCSIZE => 0,
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

    let chrome_style_mask: isize = (WS_CAPTION
        | WS_THICKFRAME
        | WS_BORDER
        | WS_DLGFRAME) as isize;
    let required_style_bits: isize =
        (WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX) as isize;
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
fn restart_embedded_gateway(
    handle: AppHandle,
    state: State<'_, ServerState>,
) -> Result<(), String> {
    {
        let guard = state.0.lock();
        if guard.bootstrap_in_progress {
            return Err(
                "gateway bootstrap is already in progress; ignoring restart request".to_string(),
            );
        }
        if let Some(started) = guard.last_bootstrap_started_at {
            if started.elapsed() < Duration::from_secs(RESTART_DEBOUNCE_SECS) {
                return Err(format!(
                    "gateway was (re)started less than {}s ago; refusing duplicate restart",
                    RESTART_DEBOUNCE_SECS
                ));
            }
        }
    }
    spawn_gateway_bootstrap_thread(handle, state.inner().clone())
}

#[tauri::command]
fn prepare_for_update_install(handle: AppHandle) -> Result<(), String> {
    process_lifetime::run_full_shutdown(&handle, Duration::from_secs(8));
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
    thread::Builder::new()
        .name("sen-window-show-fallback".into())
        .spawn(move || {
            thread::sleep(Duration::from_millis(MAIN_WINDOW_SHOW_FALLBACK_MS));
            let win_for_closure = window.clone();
            let _ = window.run_on_main_thread(move || {
                show_main_window_now(&win_for_closure);
            });
        })
        .ok();
}

#[tauri::command]
fn reveal_in_explorer(path: String) -> Result<(), String> {
    use std::path::PathBuf;
    use std::process::Command as StdCommand;

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
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut cmd = StdCommand::new("explorer.exe");
        cmd.creation_flags(CREATE_NO_WINDOW);
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
        let mut cmd = StdCommand::new("open");
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
        StdCommand::new("xdg-open")
            .arg(dir_to_open.as_os_str())
            .spawn()
            .map_err(|e| format!("xdg-open spawn failed: {e}"))?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    Err("unsupported platform".to_string())
}

pub fn run() {
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
            restart_embedded_gateway,
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
        ]);

    let app = builder
        .setup(|app| {
            if let Some(main) = app.get_webview_window("main") {
                #[cfg(target_os = "windows")]
                disable_window_focus_border(&main);

                schedule_main_window_show_fallback(main.clone());
            }

            browser_dock::install_into(app.handle());
            fetch_worker::install_into(app.handle());

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
        g.bootstrap_generation = g.bootstrap_generation.saturating_add(1);
        g.bootstrap_in_progress = true;
        g.last_bootstrap_started_at = Some(Instant::now());
        g.bootstrap_generation
    };
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
            let mut g = server_state.0.lock();
            if g.bootstrap_generation == generation {
                g.bootstrap_in_progress = false;
            }
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
            let mut g = server_state.0.lock();
            if g.bootstrap_generation != generation {
                return;
            }
            g.url = Some(url);
            g.bootstrap_in_progress = false;
            tracing::info!(
                elapsed_ms = started_at.elapsed().as_millis() as u64,
                "[sen-desktop] embedded gateway is HTTP-ready"
            );
        }
        Err(err) => {
            tracing::error!(
                elapsed_ms = started_at.elapsed().as_millis() as u64,
                "[sen-desktop] embedded gateway bootstrap failed; user can request restart via UI: {err}"
            );
            let mut g = server_state.0.lock();
            if g.bootstrap_generation == generation {
                g.bootstrap_in_progress = false;
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

fn wait_for_server_until_ready(
    server_state: &ServerState,
    generation: u64,
    host: &str,
    port: u16,
) -> Result<(), String> {
    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|err| format!("parse server address: {err}"))?;
    let started = Instant::now();
    let mut last_log = Instant::now();
    let hard_deadline = started + Duration::from_secs(GATEWAY_HEALTH_DEADLINE_SECS);
    loop {
        {
            let g = server_state.0.lock();
            if g.bootstrap_generation != generation {
                return Err("bootstrap generation invalidated; abandoning wait".into());
            }
        }
        if probe_health_once(addr, GATEWAY_HEALTH_PROBE_TIMEOUT_MS) {
            tracing::info!(
                elapsed_ms = started.elapsed().as_millis() as u64,
                "[sen-desktop] /health responded OK on {host}:{port}"
            );
            return Ok(());
        }
        if last_log.elapsed() >= Duration::from_secs(15) {
            tracing::info!(
                "[sen-desktop] still waiting for embedded gateway /health on {host}:{port} ({}s elapsed)",
                started.elapsed().as_secs()
            );
            last_log = Instant::now();
        }
        if Instant::now() >= hard_deadline {
            return Err(format!(
                "embedded gateway did not respond to /health on {host}:{port} within {}s",
                GATEWAY_HEALTH_DEADLINE_SECS
            ));
        }
        thread::sleep(Duration::from_millis(GATEWAY_HEALTH_PROBE_INTERVAL_MS));
    }
}

fn start_embedded_gateway_once(
    _handle: AppHandle,
    server_state: &ServerState,
    generation: u64,
) -> Result<String, String> {
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
                let load_started = Instant::now();
                let mut config = senweavercoding::Config::load_or_init().await.unwrap_or_else(|err| {
                    tracing::warn!(
                        "[sen-desktop] config load failed ({err}); falling back to defaults"
                    );
                    senweavercoding::Config::default()
                });
                tracing::info!(
                    elapsed_ms = load_started.elapsed().as_millis() as u64,
                    "[sen-desktop] gateway: config loaded"
                );
                apply_embedded_gateway_overrides(&mut config, host, port);
                let tokio_listener = match tokio::net::TcpListener::from_std(std_listener) {
                    Ok(l) => l,
                    Err(err) => {
                        tracing::error!("[sen-desktop] failed to convert std listener to tokio: {err}");
                        return;
                    }
                };
                let serve_started = Instant::now();
                if let Err(err) =
                    senweavercoding::gateway::run_gateway_with_supervisors(host, port, config, Some(tokio_listener)).await
                {
                    tracing::error!(
                        elapsed_ms = serve_started.elapsed().as_millis() as u64,
                        "[sen-desktop] run_gateway exited: {err:#}"
                    );
                }
            });
        })
        .map_err(|err| format!("spawn gateway thread: {err}"))?;

    wait_for_server_until_ready(server_state, generation, host, port)?;
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
