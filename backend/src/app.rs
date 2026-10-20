use std::time::Duration;

use tokio::sync::{mpsc, watch};

use crate::cli::Cli;
use crate::types::WsMessage;

/// Spawns the data source and state aggregator, then serves HTTP/WS until
/// `shutdown` resolves. `shutdown` feeds `axum::serve(...).with_graceful_shutdown(...)`,
/// so the console Ctrl-C path, the Windows tray's Quit and the Windows
/// service-stop path all funnel through the same exit mechanism.
pub async fn run(
    cli: Cli,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let (update_tx, update_rx) = mpsc::channel(256);
    let session_timeout = Duration::from_secs(cli.session_timeout_secs);
    let state_config = crate::state::Config {
        session_timeout,
        set_end_fraction: cli.set_end_fraction,
        set_start_fraction: cli.set_start_fraction,
        set_min_speed_mps: cli.set_min_speed_mps,
    };

    if cli.simulate {
        tracing::info!("starting simulator data source");
        tokio::spawn(async move {
            if let Err(err) = crate::simulator::run(update_tx, session_timeout).await {
                tracing::error!(?err, "simulator source exited");
            }
        });
    } else {
        tracing::info!("starting real ANT+ USB data source");
        tokio::spawn(async move {
            if let Err(err) = crate::ant_source::run(update_tx).await {
                tracing::error!(?err, "ANT+ USB source exited");
            }
        });
    }

    let initial_state = WsMessage::State {
        set: None,
        session: None,
        devices: Vec::new(),
    };
    let (state_tx, state_rx) = watch::channel(initial_state);

    tokio::spawn(crate::state::run(update_rx, state_tx, state_config));

    let ctx = crate::ws::AppContext { rx: state_rx };
    let app = crate::ws::router(ctx);

    // Only the local browser needs to reach the dashboard.
    let addr = format!("127.0.0.1:{}", cli.port);
    tracing::info!(%addr, "listening");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;

    tracing::info!("shutdown complete");
    Ok(())
}
