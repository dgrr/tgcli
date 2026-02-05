mod app;
mod cmd;
mod error;
mod out;
mod shutdown;
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

    // Set up global shutdown handler
    let shutdown = shutdown::ShutdownController::new();
    shutdown::set_global(shutdown.clone());

    // Spawn signal handler task
    let shutdown_clone = shutdown.clone();
    tokio::spawn(async move {
        if let Ok(()) = tokio::signal::ctrl_c().await {
            log::info!("Received Ctrl+C, initiating graceful shutdown...");
            shutdown_clone.trigger();
        }
    });

    if let Err(e) = cmd::run(cli).await {
        // Don't report error if we're shutting down gracefully
        if shutdown.is_triggered() {
            std::process::exit(0);
        }
        let msg = format!("{e:#}");
        eprintln!("Error: {msg}");
        std::process::exit(1);
    }
}
