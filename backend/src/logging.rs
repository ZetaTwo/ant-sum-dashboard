/// Console/Linux/interactive-Windows path: unchanged from before this
/// module existed — stdout, default `tracing_subscriber::fmt` format.
pub fn init_console() {
    tracing_subscriber::fmt::init();
}

/// Windows-service path: no console is attached, so logs go to a
/// daily-rolling file instead. The returned `WorkerGuard` must be kept
/// alive for the process lifetime — dropping it early silently stops log
/// writes (the non-blocking writer flushes on drop).
#[cfg(windows)]
pub fn init_service(log_dir: &std::path::Path) -> tracing_appender::non_blocking::WorkerGuard {
    let file_appender = tracing_appender::rolling::daily(log_dir, "ant-sum-dashboard.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_ansi(false)
        .init();
    guard
}
