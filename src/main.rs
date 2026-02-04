mod app;
mod cmd;
mod out;
mod store;
mod tg;

use clap::Parser;
use std::time::Duration;

#[derive(Parser, Debug, Clone)]
#[command(name = "tgrs", version, about = "Telegram CLI (pure Rust, no TDLib)")]
pub struct Cli {
    /// Store directory (default: ~/.tgrs)
    #[arg(long, global = true, default_value = "~/.tgrs")]
    pub store: String,

    /// Output JSON instead of human-readable text
    #[arg(long, global = true, default_value_t = false)]
    pub json: bool,

    /// Command timeout in seconds (non-sync commands)
    #[arg(long, global = true, default_value = "300")]
    pub timeout: u64,

    #[command(subcommand)]
    pub command: cmd::Command,
}

impl Cli {
    pub fn store_dir(&self) -> String {
        let s = &self.store;
        if s.starts_with("~/") {
            if let Some(home) = dirs_home() {
                return format!("{}{}", home, &s[1..]);
            }
        }
        s.clone()
    }

    pub fn timeout_duration(&self) -> Duration {
        Duration::from_secs(self.timeout)
    }
}

fn dirs_home() -> Option<String> {
    std::env::var("HOME").ok()
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let cli = Cli::parse();

    if let Err(e) = cmd::run(cli).await {
        let msg = format!("{e:#}");
        eprintln!("Error: {msg}");
        std::process::exit(1);
    }
}
