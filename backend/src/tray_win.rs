#![cfg(windows)]
//! On Windows the dashboard runs as a tray application in the user's
//! session: the server runs on a background thread while the main thread
//! pumps the Win32 messages the tray icon and its menu depend on.

use std::time::Duration;

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};
use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MB_ICONERROR, MB_OK, MSG, MessageBoxW, PostThreadMessageW,
    SW_SHOWNORMAL, TranslateMessage, WM_QUIT,
};

use crate::cli::Cli;

const APP_NAME: &str = "ANT+ Sum Dashboard";

/// The binary uses the GUI subsystem so double-clicking it doesn't open a
/// console window. When launched from a terminal, this reattaches to that
/// terminal so `--help` and log output still show up there.
pub fn attach_parent_console() -> bool {
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) != 0 }
}

pub fn run(cli: Cli, has_console: bool) -> anyhow::Result<()> {
    let log_dir = cli.log_dir.clone().unwrap_or_else(default_log_dir);
    std::fs::create_dir_all(&log_dir)?;
    let _guard = crate::logging::init_windows(&log_dir, has_console);
    tracing::info!(log_dir = %log_dir.display(), "starting tray application");

    let result = run_tray(cli);
    if let Err(err) = &result {
        tracing::error!(?err, "exiting");
    }
    result
}

fn run_tray(cli: Cli) -> anyhow::Result<()> {
    let url = format!("http://127.0.0.1:{}/", cli.port);
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

    let open_item = MenuItem::new("Open dashboard", true, None);
    let quit_item = MenuItem::new("Quit", true, None);
    let menu = Menu::with_items(&[
        &open_item,
        &MenuItem::new(&url, false, None),
        &PredefinedMenuItem::separator(),
        &quit_item,
    ])?;
    // Must stay alive for the icon to remain in the tray.
    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(APP_NAME)
        .with_icon(placeholder_icon())
        .build()?;

    let (open_id, quit_id) = (open_item.id().clone(), quit_item.id().clone());
    let menu_url = url.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == open_id {
            open_in_browser(&menu_url);
        } else if event.id == quit_id {
            let _ = shutdown_tx.send(true);
        }
    }));

    // The tray icon's window above created this thread's message queue, so
    // the server thread can always post WM_QUIT to it.
    let ui_thread = unsafe { GetCurrentThreadId() };
    let server = std::thread::spawn(move || {
        let result = serve(cli, async move {
            let _ = shutdown_rx.wait_for(|stopped| *stopped).await;
        });
        unsafe { PostThreadMessageW(ui_thread, WM_QUIT, 0, 0) };
        result
    });

    pump_messages();
    server
        .join()
        .map_err(|_| anyhow::anyhow!("server thread panicked"))?
}

fn serve(
    cli: Cli,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    let result = rt.block_on(crate::app::run(cli, shutdown));
    // Bounded: the blocking USB thread only notices shutdown on its next poll.
    rt.shutdown_timeout(Duration::from_secs(5));
    result
}

fn pump_messages() {
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn open_in_browser(url: &str) {
    let (verb, url) = (wide("open"), wide(url));
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            url.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
}

/// There's no console to report errors to when launched normally, so
/// fatal ones are shown in a dialog.
pub fn show_error(message: &str) {
    let (text, caption) = (wide(message), wide(APP_NAME));
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONERROR,
        )
    };
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Placeholder until there's a real icon: a filled circle in the
/// frontend's accent color.
fn placeholder_icon() -> Icon {
    const SIZE: u32 = 32;
    const COLOR: [u8; 4] = [0x4f, 0xd1, 0xc5, 0xff];
    let center = (SIZE as f32 - 1.0) / 2.0;
    let radius = SIZE as f32 / 2.0 - 1.0;
    let rgba = (0..SIZE * SIZE)
        .flat_map(|i| {
            let (x, y) = ((i % SIZE) as f32, (i / SIZE) as f32);
            let inside = (x - center).hypot(y - center) <= radius;
            if inside { COLOR } else { [0; 4] }
        })
        .collect();
    Icon::from_rgba(rgba, SIZE, SIZE).expect("placeholder icon dimensions match its buffer")
}

fn default_log_dir() -> std::path::PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("ant-sum-dashboard")
        .join("logs")
}
