mod ant_source;
mod cli;
mod fe_c;
mod simulator;
mod state;
mod types;
mod ws;

use std::time::Duration;

use clap::Parser;
use tokio::sync::{mpsc, watch};

use crate::types::WsMessage;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = cli::Cli::parse();
    let (update_tx, update_rx) = mpsc::channel(256);
    let inactivity_timeout = Duration::from_secs(cli.inactivity_timeout_secs);

    if cli.simulate {
        tracing::info!("starting simulator data source");
        tokio::spawn(async move {
            if let Err(err) = simulator::run(update_tx, inactivity_timeout).await {
                tracing::error!(?err, "simulator source exited");
            }
        });
    } else {
        tracing::info!("starting real ANT+ USB data source");
        tokio::spawn(async move {
            if let Err(err) = ant_source::run(update_tx).await {
                tracing::error!(?err, "ANT+ USB source exited");
            }
        });
    }

    let initial_state = WsMessage::State {
        total_distance_m: 0.0,
        session_started_at_ms: None,
        devices: Vec::new(),
    };
    let (state_tx, state_rx) = watch::channel(initial_state);

    tokio::spawn(state::run(update_rx, state_tx, inactivity_timeout));

    let ctx = ws::AppContext { rx: state_rx };
    let app = ws::router(ctx);

    let addr = format!("0.0.0.0:{}", cli.port);
    tracing::info!(%addr, "listening");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
