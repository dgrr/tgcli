//! Daemon service management (launchd on macOS, systemd on Linux).
//!
//! Provides commands to install, start, stop, and check status of tgcli as a background service.

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Generate a service domain name from store path
fn generate_service_domain(store: &str) -> String {
    // Expand ~ to home directory first
    let expanded = shellexpand::tilde(store).to_string();

    // Extract just the filename (e.g., ~/.tgcli-uae -> tgcli-uae)
    let filename = PathBuf::from(&expanded)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("tgcli")
        .to_string();

    if filename == "tgcli" || filename == ".tgcli" {
        // Default personal account
        "com.tgcli.sync".to_string()
    } else if filename.starts_with(".tgcli-") {
        // Additional accounts like ~/.tgcli-uae -> com.tgcli.sync.uae
        // Replace all - with . for valid launchd domain
        let suffix = filename.trim_start_matches(".tgcli-").replace('-', ".");
        format!("com.tgcli.sync.{}", suffix)
    } else {
        // Fallback for custom paths - replace - and . with _
        format!("com.tgcli.{}", filename.replace(['-', '.'], "_"))
    }
}

/// Detect the current platform
fn detect_platform() -> &'static str {
    match env::consts::OS {
        "macos" => "macos",
        "linux" => "linux",
        _ => "unknown",
    }
}

#[derive(Args, Debug, Clone)]
pub struct DaemonServiceArgs {
    #[command(subcommand)]
    pub command: DaemonServiceSubcommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum DaemonServiceSubcommand {
    /// Install tgcli as a background service (launchd on macOS, systemd on Linux)
    Install(DaemonInstallArgs),
    /// Start the background service
    Start,
    /// Stop the background service
    Stop,
    /// Restart the background service
    Restart,
    /// Uninstall and remove the background service
    Uninstall(UninstallArgs),
    /// Check if the service is running
    Status,
}

#[derive(Args, Debug, Clone)]
pub struct DaemonInstallArgs {
    /// Store directory (e.g., ~/.tgcli or ~/.tgcli-uae)
    #[arg(long, default_value = "~/.tgcli")]
    pub store: String,

    /// Don't run backfill on startup
    #[arg(long, default_value_t = true)]
    pub no_backfill: bool,

    /// Run with --quiet flag
    #[arg(long, default_value_t = false)]
    pub quiet: bool,

    /// Chat IDs to ignore (can be specified multiple times)
    #[arg(long = "ignore", value_name = "CHAT_ID")]
    pub ignore_chat_ids: Vec<i64>,
}

#[derive(Args, Debug, Clone)]
pub struct UninstallArgs {
    /// Specific service domain to uninstall (e.g., com.tgcli.sync.uae)
    /// If not provided, uninstalls all installed services
    #[arg(long)]
    pub domain: Option<String>,

    /// Force uninstall without confirmation
    #[arg(long, default_value_t = false)]
    pub force: bool,
}

pub async fn run(cli: &crate::Cli, args: &DaemonServiceArgs) -> Result<()> {
    let platform = detect_platform();

    match &args.command {
        DaemonServiceSubcommand::Install(install_args) => {
            install_service(cli, install_args, platform).await
        }
        DaemonServiceSubcommand::Start => start_service(platform).await,
        DaemonServiceSubcommand::Stop => stop_service(platform).await,
        DaemonServiceSubcommand::Restart => restart_service(platform).await,
        DaemonServiceSubcommand::Uninstall(uninstall_args) => {
            uninstall_service(uninstall_args, platform).await
        }
        DaemonServiceSubcommand::Status => status_service(platform).await,
    }
}

async fn install_service(
    _cli: &crate::Cli,
    args: &DaemonInstallArgs,
    platform: &str,
) -> Result<()> {
    let store = shellexpand::tilde(&args.store).to_string();
    let binary_path = std::env::current_exe()?
        .parent()
        .expect("binary path should have parent")
        .join("tgcli")
        .display()
        .to_string();

    // Build command arguments
    let mut cmd_args = vec!["--store".to_string(), store.clone(), "daemon".to_string()];

    if args.no_backfill {
        cmd_args.push("--no-backfill".to_string());
    } else {
        cmd_args.push("--no-backfill=false".to_string());
    }

    if args.quiet {
        cmd_args.push("--quiet".to_string());
    }

    // Add ignore chat IDs
    for id in &args.ignore_chat_ids {
        cmd_args.push("--ignore".to_string());
        cmd_args.push(id.to_string());
    }

    match platform {
        "macos" => install_launchd(&binary_path, &store, &cmd_args).await,
        "linux" => install_systemd(&binary_path, &store, &cmd_args).await,
        _ => Err(anyhow::anyhow!(
            "Unsupported platform: {}. Only macOS and Linux are supported.",
            platform
        )),
    }
}

/// Install as launchd service on macOS
async fn install_launchd(binary_path: &str, store: &str, cmd_args: &[String]) -> Result<()> {
    let domain = generate_service_domain(store);

    // Create log directory
    let store_path = shellexpand::tilde(store).to_string();
    let log_dir = PathBuf::from(&store_path).join("logs");
    fs::create_dir_all(&log_dir)?;
    let log_base = log_dir.join("daemon");

    let plist_content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{domain}</string>
    
    <key>ProgramArguments</key>
    <array>
        <string>{binary}</string>
        {program_args}
    </array>
    
    <key>RunAtLoad</key>
    <true/>
    
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
        <key>Crashed</key>
        <true/>
    </dict>
    
    <key>StandardOutPath</key>
    <string>{log_path}.out</string>
    
    <key>StandardErrorPath</key>
    <string>{log_path}.err</string>
    
    <key>EnvironmentVariables</key>
    <dict>
        <key>RUST_LOG</key>
        <string>info</string>
    </dict>
</dict>
</plist>
"#,
        domain = domain,
        binary = binary_path,
        program_args = cmd_args
            .iter()
            .map(|arg| format!("        <string>{}</string>", arg))
            .collect::<Vec<_>>()
            .join("\n"),
        log_path = log_base.display()
    );

    // Determine plist destination
    let plist_dir = dirs::home_dir()
        .expect("Home directory not found")
        .join("Library/LaunchAgents");

    fs::create_dir_all(&plist_dir)?;

    let plist_path = plist_dir.join(format!("{}.plist", domain));
    fs::write(&plist_path, &plist_content)?;

    println!("✓ Service plist created at: {}", plist_path.display());

    // Load and start the service
    let output = Command::new("launchctl")
        .args(["load", "-w", &plist_path.to_string_lossy()])
        .output()
        .context("Failed to execute launchctl load")?;

    if !output.status.success() {
        eprintln!(
            "Warning: launchctl load failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    } else {
        println!("✓ Service installed and started: {}", domain);
    }

    Ok(())
}

/// Install as systemd service on Linux
async fn install_systemd(binary_path: &str, store: &str, cmd_args: &[String]) -> Result<()> {
    let domain = generate_service_domain(store);

    // Create log directory
    let store_path = shellexpand::tilde(store).to_string();
    let log_dir = PathBuf::from(&store_path).join("logs");
    fs::create_dir_all(&log_dir)?;
    let log_base = log_dir.join("daemon");

    let service_content = format!(
        r#"[Unit]
Description=Telegram CLI Daemon ({})
After=network.target

[Service]
Type=simple
ExecStart={} {}
Restart=always
RestartSec=5
StandardOutput=append:{}
StandardError=append:{}
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
"#,
        domain,
        binary_path,
        cmd_args.join(" "),
        log_base.with_extension("out").display(),
        log_base.with_extension("err").display()
    );

    // Determine systemd user directory
    let systemd_dir = dirs::home_dir()
        .expect("Home directory not found")
        .join(".config/systemd/user");

    fs::create_dir_all(&systemd_dir)?;

    let service_path = systemd_dir.join(format!("{}.service", domain));
    fs::write(&service_path, &service_content)?;

    println!("✓ Service file created at: {}", service_path.display());

    // Reload systemd daemon
    let reload_output = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output()?;

    if !reload_output.status.success() {
        eprintln!(
            "Warning: systemctl daemon-reload failed: {}",
            String::from_utf8_lossy(&reload_output.stderr)
        );
    }

    // Enable and start the service
    let enable_output = Command::new("systemctl")
        .args(["--user", "enable", "--now", &domain])
        .output()?;

    if !enable_output.status.success() {
        eprintln!(
            "Warning: systemctl enable/start failed: {}",
            String::from_utf8_lossy(&enable_output.stderr)
        );
    } else {
        println!("✓ Service installed and started: {}", domain);
    }

    Ok(())
}

async fn start_service(platform: &str) -> Result<()> {
    let domains = get_installed_domains(platform)?;

    if domains.is_empty() {
        println!("No services installed");
        return Ok(());
    }

    for domain in &domains {
        let output = match platform {
            "macos" => Command::new("launchctl").args(["start", domain]).output()?,
            "linux" => Command::new("systemctl")
                .args(["--user", "start", domain])
                .output()?,
            _ => continue,
        };

        if output.status.success() {
            println!("✓ Started: {}", domain);
        } else {
            eprintln!(
                "⚠ Failed to start {}: {}",
                domain,
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    Ok(())
}

async fn stop_service(platform: &str) -> Result<()> {
    let domains = get_installed_domains(platform)?;

    if domains.is_empty() {
        println!("No services installed");
        return Ok(());
    }

    for domain in &domains {
        let output = match platform {
            "macos" => Command::new("launchctl").args(["stop", domain]).output()?,
            "linux" => Command::new("systemctl")
                .args(["--user", "stop", domain])
                .output()?,
            _ => continue,
        };

        if output.status.success() {
            println!("✓ Stopped: {}", domain);
        } else {
            eprintln!(
                "⚠ Failed to stop {}: {}",
                domain,
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    Ok(())
}

async fn restart_service(platform: &str) -> Result<()> {
    let domains = get_installed_domains(platform)?;

    if domains.is_empty() {
        println!("No services installed");
        return Ok(());
    }

    for domain in &domains {
        // Stop first
        let stop_output = match platform {
            "macos" => Command::new("launchctl").args(["stop", domain]).output()?,
            "linux" => Command::new("systemctl")
                .args(["--user", "stop", domain])
                .output()?,
            _ => continue,
        };

        if !stop_output.status.success() {
            eprintln!(
                "⚠ Failed to stop {}: {}",
                domain,
                String::from_utf8_lossy(&stop_output.stderr)
            );
            continue;
        }

        // Small delay to ensure cleanup
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // Then start
        let start_output = match platform {
            "macos" => Command::new("launchctl").args(["start", domain]).output()?,
            "linux" => Command::new("systemctl")
                .args(["--user", "start", domain])
                .output()?,
            _ => continue,
        };

        if start_output.status.success() {
            println!("✓ Restarted: {}", domain);
        } else {
            eprintln!(
                "⚠ Failed to start {}: {}",
                domain,
                String::from_utf8_lossy(&start_output.stderr)
            );
        }
    }

    Ok(())
}

async fn status_service(platform: &str) -> Result<()> {
    let domains = get_installed_domains(platform)?;

    if domains.is_empty() {
        println!("No services installed");
        return Ok(());
    }

    for domain in &domains {
        let output = match platform {
            "macos" => Command::new("launchctl").args(["list", domain]).output()?,
            "linux" => Command::new("systemctl")
                .args(["--user", "is-active", domain])
                .output()?,
            _ => continue,
        };

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            println!("✓ {} is running", domain);
            if platform == "macos" {
                println!("  {}", stdout.lines().next().unwrap_or(""));
            }
        } else {
            println!("✗ {} is not running", domain);
        }
    }

    Ok(())
}

async fn uninstall_service(args: &UninstallArgs, platform: &str) -> Result<()> {
    let config_dir = match platform {
        "macos" => dirs::home_dir()
            .expect("Home directory not found")
            .join("Library/LaunchAgents"),
        "linux" => dirs::home_dir()
            .expect("Home directory not found")
            .join(".config/systemd/user"),
        _ => return Err(anyhow::anyhow!("Unsupported platform")),
    };

    let domains_to_uninstall = if let Some(domain) = &args.domain {
        vec![domain.clone()]
    } else {
        get_installed_domains(platform)?
    };

    if domains_to_uninstall.is_empty() {
        println!("No services to uninstall");
        return Ok(());
    }

    for domain in &domains_to_uninstall {
        let file_path = config_dir.join(format!(
            "{}.{}",
            domain,
            if platform == "macos" {
                "plist"
            } else {
                "service"
            }
        ));

        if !file_path.exists() {
            println!("⚠ File not found: {}", file_path.display());
            continue;
        }

        // Stop the service first
        let _ = match platform {
            "macos" => Command::new("launchctl").args(["stop", domain]).output(),
            "linux" => Command::new("systemctl")
                .args(["--user", "stop", domain])
                .output(),
            _ => return Err(anyhow::anyhow!("Unsupported platform")),
        };

        // Unload/disable the service
        let unload_output = match platform {
            "macos" => Command::new("launchctl")
                .args(["unload", "-w", &file_path.to_string_lossy()])
                .output()?,
            "linux" => Command::new("systemctl")
                .args(["--user", "disable", "--now", domain])
                .output()?,
            _ => continue,
        };

        if !unload_output.status.success() {
            eprintln!(
                "⚠ Failed to unload {}: {}",
                domain,
                String::from_utf8_lossy(&unload_output.stderr)
            );
        }

        // Remove file
        match fs::remove_file(&file_path) {
            Ok(_) => {
                println!("✓ Uninstalled: {}", domain);
                println!("  Removed: {}", file_path.display());
            }
            Err(e) => {
                eprintln!("⚠ Failed to remove file for {}: {}", domain, e);
            }
        }

        // Reload daemon on Linux
        if platform == "linux" {
            let _ = Command::new("systemctl")
                .args(["--user", "daemon-reload"])
                .output();
        }
    }

    Ok(())
}

/// Get list of installed service domains
fn get_installed_domains(platform: &str) -> Result<Vec<String>> {
    let config_dir = match platform {
        "macos" => dirs::home_dir()
            .expect("Home directory not found")
            .join("Library/LaunchAgents"),
        "linux" => dirs::home_dir()
            .expect("Home directory not found")
            .join(".config/systemd/user"),
        _ => return Ok(vec![]),
    };

    let mut domains = Vec::new();
    let extension = if platform == "macos" {
        "plist"
    } else {
        "service"
    };

    if let Ok(entries) = fs::read_dir(&config_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == extension) {
                if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                    if filename.starts_with("com.tgcli.")
                        && filename.ends_with(&format!(".{}", extension))
                    {
                        let domain = filename.trim_end_matches(&format!(".{}", extension));
                        domains.push(domain.to_string());
                    }
                }
            }
        }
    }

    Ok(domains)
}
