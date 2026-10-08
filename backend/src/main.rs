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

use clap::Parser;
use cli::Cli;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    #[cfg(windows)]
    {
        // If this process was launched by the Windows Service Control
        // Manager, this blocks until the service stops and returns Ok(()).
        // If not (console, debugger, double-click), it fails fast and we
        // fall through to the normal interactive path below.
        if service_win::try_run_as_service(cli.clone()).is_ok() {
            return Ok(());
        }
    }

    run_console(cli)
}

fn run_console(cli: Cli) -> anyhow::Result<()> {
    logging::init_console();
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(app::run(cli, async {
        let _ = tokio::signal::ctrl_c().await;
    }))
}
