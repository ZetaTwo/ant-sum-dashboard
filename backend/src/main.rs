#![cfg_attr(windows, windows_subsystem = "windows")]

mod ant_source;
mod app;
mod cli;
mod fe_c;
mod logging;
mod simulator;
mod state;
mod types;
mod ws;

#[cfg(windows)]
mod service_win;
#[cfg(windows)]
mod tray_win;

use clap::Parser;
use cli::Cli;

#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    // Before parsing, so `--help` and usage errors reach the terminal this
    // was launched from, if any.
    let has_console = tray_win::attach_parent_console();
    let cli = Cli::parse();
    // If this process was launched by the Windows Service Control Manager,
    // this blocks until the service stops and returns Ok(()). If not
    // (double-click, terminal, shortcut), it fails fast and we fall through
    // to the tray application.
    if service_win::try_run_as_service(cli.clone()).is_ok() {
        return Ok(());
    }
    let result = tray_win::run(cli, has_console);
    if let Err(err) = &result {
        tracing::error!(?err, "exiting");
        tray_win::show_error(&format!("Stopped with an error:\n\n{err:#}"));
    }
    result
}

#[cfg(not(windows))]
fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    logging::init_console();
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(app::run(cli, async {
        let _ = tokio::signal::ctrl_c().await;
    }))
}
