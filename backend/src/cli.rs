use clap::Parser;

#[derive(Parser, Debug, Clone)]
pub struct Cli {
    #[arg(long, env = "PORT", default_value_t = 8080)]
    pub port: u16,

    /// Rest after the last set before the session ends and resets.
    #[arg(long, env = "SESSION_TIMEOUT_SECS", default_value_t = 300)]
    pub session_timeout_secs: u64,

    /// A set ends once activity drops below this fraction of its recent
    /// average.
    #[arg(long, env = "SET_END_FRACTION", default_value_t = 0.25)]
    pub set_end_fraction: f64,

    /// A new set starts once activity exceeds this fraction of the previous
    /// set's average. Keep it above `--set-end-fraction`.
    #[arg(long, env = "SET_START_FRACTION", default_value_t = 0.4)]
    pub set_start_fraction: f64,

    /// Minimum mean speed per connected device (m/s) to start a set.
    #[arg(long, env = "SET_MIN_SPEED_MPS", default_value_t = 0.5)]
    pub set_min_speed_mps: f64,

    /// Use a synthetic data source instead of a real ANT+ USB dongle.
    #[arg(long, env = "SIMULATE")]
    pub simulate: bool,

    /// Directory for rolling log files on Windows (ignored elsewhere).
    /// Defaults to `%LOCALAPPDATA%\ant-sum-dashboard\logs` for the tray
    /// app and `%ProgramData%\ant-sum-dashboard\logs` as a service.
    #[cfg(windows)]
    #[arg(long, env = "LOG_DIR")]
    pub log_dir: Option<std::path::PathBuf>,
}
