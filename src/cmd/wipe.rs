use crate::out;
use crate::Cli;
use anyhow::Result;
use clap::Args;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Args, Debug, Clone)]
pub struct WipeArgs {
    /// Skip confirmation prompt
    #[arg(long, short = 'y')]
    pub yes: bool,
}

pub async fn run(cli: &Cli, args: &WipeArgs) -> Result<()> {
    let store_dir = cli.store_dir();
    let db_path = PathBuf::from(&store_dir).join("tgcli.db");

    if !db_path.exists() {
        if cli.output.is_json() {
            out::write_json(&serde_json::json!({
                "wiped": false,
                "reason": "database does not exist"
            }))?;
        } else {
            println!("Nothing to wipe (tgcli.db does not exist).");
        }
        return Ok(());
    }

    // Get size for display
    let db_size = fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);

    // Show what will be deleted and confirm
    if !cli.output.is_json() && !args.yes {
        println!("This will delete tgcli.db ({}).", format_size(db_size));
        println!("Session and media will be preserved.");
        println!();
        print!("Are you sure? [y/N] ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim().to_lowercase();

        if input != "y" && input != "yes" {
            println!("Aborted.");
            return Ok(());
        }
    }

    // Delete database
    fs::remove_file(&db_path)?;

    // Also remove WAL/SHM files if present
    let wal_path = PathBuf::from(&store_dir).join("tgcli.db-wal");
    let shm_path = PathBuf::from(&store_dir).join("tgcli.db-shm");
    let _ = fs::remove_file(&wal_path);
    let _ = fs::remove_file(&shm_path);

    if cli.output.is_json() {
        out::write_json(&serde_json::json!({
            "wiped": true,
            "deleted_size": db_size
        }))?;
    } else {
        println!("Wiped tgcli.db ({}).", format_size(db_size));
    }

    Ok(())
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}
