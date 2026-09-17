use clap::Parser;

#[derive(Parser, Debug)]
pub struct Cli {
    #[arg(long, env = "PORT", default_value_t = 8080)]
    pub port: u16,

    #[arg(long, env = "INACTIVITY_TIMEOUT_SECS", default_value_t = 30)]
    pub inactivity_timeout_secs: u64,

    /// Use a synthetic data source instead of a real ANT+ USB dongle.
    #[arg(long, env = "SIMULATE")]
    pub simulate: bool,

    #[arg(long, env = "STATIC_DIR", default_value = "../frontend/dist")]
    pub static_dir: String,
}
