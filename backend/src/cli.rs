use clap::Parser;

#[derive(Parser, Debug, Clone)]
pub struct Cli {
    #[arg(long, env = "PORT", default_value_t = 8080)]
    pub port: u16,

    #[arg(long, env = "INACTIVITY_TIMEOUT_SECS", default_value_t = 30)]
    pub inactivity_timeout_secs: u64,

    /// Use a synthetic data source instead of a real ANT+ USB dongle.
    #[arg(long, env = "SIMULATE")]
    pub simulate: bool,

    /// Directory for rolling log files when running as a Windows service
    /// (ignored otherwise). Defaults to `%ProgramData%\ant-sum-dashboard\logs`.
    #[cfg(windows)]
    #[arg(long, env = "LOG_DIR")]
    pub log_dir: Option<std::path::PathBuf>,
}
