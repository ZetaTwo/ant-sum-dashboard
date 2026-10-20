/// Console/Linux path: stdout, default `tracing_subscriber::fmt` format.
#[cfg(not(windows))]
pub fn init_console() {
    tracing_subscriber::fmt::init();
}

/// Daily-rolling file, plus stdout when attached to a terminal. Keep the
/// guard alive: dropping it stops file writes.
#[cfg(windows)]
pub fn init_windows(
    log_dir: &std::path::Path,
    console: bool,
) -> tracing_appender::non_blocking::WorkerGuard {
    use tracing_subscriber::filter::{EnvFilter, LevelFilter};
    use tracing_subscriber::prelude::*;

    let file_appender = tracing_appender::rolling::daily(log_dir, "ant-sum-dashboard.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::registry()
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false),
        )
        .with(console.then(tracing_subscriber::fmt::layer))
        .init();
    guard
}
