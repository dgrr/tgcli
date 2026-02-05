mod app;
mod cmd;
mod error;
mod out;
mod store;
mod tg;

use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(name = "tgcli", version, about = "Telegram CLI (pure Rust, no TDLib)")]
pub struct Cli {
    /// Store directory (default: ~/.tgcli)
    #[arg(long, global = true, default_value = "~/.tgcli")]
    pub store: String,

    /// Output mode: text (default), json, or none
    #[arg(long, global = true, value_enum, default_value = "text")]
    pub output: out::OutputMode,

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
