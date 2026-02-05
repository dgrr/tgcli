use crate::out;
use crate::Cli;
use anyhow::Result;
use clap::Args;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Args, Debug, Clone)]
pub struct WipeArgs {
    /// Also delete the media directory
    #[arg(long)]
    pub media: bool,

    /// Skip confirmation prompt
    #[arg(long, short = 'y')]
    pub yes: bool,
}

pub async fn run(cli: &Cli, args: &WipeArgs) -> Result<()> {
    let store_dir = cli.store_dir();
    let db_path = PathBuf::from(&store_dir).join("tgcli.db");
    let media_path = PathBuf::from(&store_dir).join("media");

    let db_exists = db_path.exists();
    let media_exists = args.media && media_path.exists();

    if !db_exists && !media_exists {
        if cli.json {
            out::write_json(&serde_json::json!({
                "wiped": false,
                "reason": "nothing to wipe"
            }))?;
        } else {
            println!("Nothing to wipe.");
        }
        return Ok(());
    }

    // Get sizes for display
    let db_size = if db_exists {
        fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0)
    } else {
        0
    };

    let media_size = if media_exists {
        dir_size(&media_path).unwrap_or(0)
    } else {
        0
    };

    // Show what will be deleted and confirm
    if !cli.json && !args.yes {
        println!("This will delete:");
        if db_exists {
            println!("  - tgcli.db ({})", format_size(db_size));
        }
        if media_exists {
            println!("  - media/ directory ({})", format_size(media_size));
        }
        println!();
        println!("Session will be preserved (session.db).");
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

    // Perform deletion
    let mut deleted_db = false;
    let mut deleted_media = false;

    if db_exists {
        fs::remove_file(&db_path)?;
        deleted_db = true;
    }

    if media_exists {
        fs::remove_dir_all(&media_path)?;
        deleted_media = true;
    }

    if cli.json {
        out::write_json(&serde_json::json!({
            "wiped": true,
            "deleted": {
                "database": deleted_db,
                "database_size": db_size,
                "media": deleted_media,
                "media_size": media_size
            }
        }))?;
    } else {
        println!("Wiped:");
        if deleted_db {
            println!("  - tgcli.db ({})", format_size(db_size));
        }
        if deleted_media {
            println!("  - media/ directory ({})", format_size(media_size));
        }
    }

    Ok(())
}

fn dir_size(path: &PathBuf) -> Result<u64> {
    let mut total = 0;
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                total += dir_size(&path)?;
            } else {
                total += entry.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    Ok(total)
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
