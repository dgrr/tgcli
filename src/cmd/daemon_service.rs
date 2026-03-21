//! Daemon service management (launchd on macOS).
//!
//! Provides commands to install, start, stop, and check status of tgcli as a background service.

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Generate a launchd domain name from store path
fn generate_service_domain(store: &str) -> String {
    let normalized = store.replace("~/", "").replace("/", "_").replace(".", "_");
    
    if normalized == "tgcli" || normalized == "_tgcli" {
        // Default personal account
        "com.tgcli.sync".to_string()
    } else if normalized.starts_with("_tgcli_") {
        // Additional accounts like ~/.tgcli-uae -> com.tgcli.sync.uae
        let suffix = normalized.trim_start_matches("_tgcli_");
        format!("com.tgcli.sync.{}", suffix)
    } else {
        // Fallback for custom paths
        format!("com.tgcli.{}", normalized)
    }
}

#[derive(Args, Debug, Clone)]
pub struct DaemonServiceArgs {
    #[command(subcommand)]
    pub command: DaemonServiceSubcommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum DaemonServiceSubcommand {
    /// Install tgcli as a background service (launchd on macOS)
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
pub struct UninstallArgs {
    /// Specific service domain to uninstall (e.g., com.tgcli.sync.uae)
    /// If not provided, uninstalls all installed services
    #[arg(long)]
    pub domain: Option<String>,
    
    /// Force uninstall without confirmation
    #[arg(long, default_value_t = false)]
    pub force: bool,
}

#[derive(Args, Debug, Clone)]
pub struct DaemonInstallArgs {
    /// Store directory (e.g., ~/.tgcli or ~/.tgcli-uae)
    #[arg(long, default_value = "~/.tgcli")]
    pub store: String,

    /// Enable RPC server
    #[arg(long, default_value_t = true)]
    pub rpc: bool,

    /// RPC server address
    #[arg(long, default_value = "127.0.0.1:5556")]
    pub rpc_addr: String,

    /// Don't run backfill on startup
    #[arg(long, default_value_t = true)]
    pub no_backfill: bool,

    /// Run with --quiet flag
    #[arg(long, default_value_t = false)]
    pub quiet: bool,
}

pub async fn run(cli: &crate::Cli, args: &DaemonServiceArgs) -> Result<()> {
    match &args.command {
        DaemonServiceSubcommand::Install(install_args) => install_service(cli, install_args).await,
        DaemonServiceSubcommand::Start => start_service().await,
        DaemonServiceSubcommand::Stop => stop_service().await,
        DaemonServiceSubcommand::Restart => restart_service().await,
        DaemonServiceSubcommand::Uninstall(uninstall_args) => uninstall_service(uninstall_args).await,
        DaemonServiceSubcommand::Status => status_service().await,
    }
}

async fn install_service(_cli: &crate::Cli, args: &DaemonInstallArgs) -> Result<()> {
    let store = shellexpand::tilde(&args.store).to_string();
    let binary_path = std::env::current_exe()?
        .parent()
        .expect("binary path should have parent")
        .join("tgcli")
        .display()
        .to_string();

    // Build command arguments
    let mut cmd_args = vec![
        "--store".to_string(),
        store.clone(),
        "daemon".to_string(),
    ];

    if args.rpc {
        cmd_args.push("--rpc".to_string());
        cmd_args.push("--rpc-addr".to_string());
        cmd_args.push(args.rpc_addr.clone());
    }

    if args.no_backfill {
        cmd_args.push("--no-backfill".to_string());
    } else {
        cmd_args.push("--no-backfill=false".to_string());
    }

    if args.quiet {
        cmd_args.push("--quiet".to_string());
    }

    // Create launchd plist
    let domain = generate_service_domain(&store);

    let plist_content = format!(r#"<?xml version="1.0" encoding="UTF-8"?>
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
        program_args = cmd_args.iter().map(|arg| format!("        <string>{}</string>", arg)).collect::<Vec<_>>().join("\n"),
        log_path = format!("/tmp/tgcli_{}", store.replace("~/", "").replace("/", "_"))
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
        eprintln!("Warning: launchctl load failed: {}", 
            String::from_utf8_lossy(&output.stderr));
    } else {
        println!("✓ Service installed and started: {}", domain);
    }

    Ok(())
}

async fn start_service() -> Result<()> {
    let domains = get_installed_domains()?;
    
    if domains.is_empty() {
        println!("No services installed");
        return Ok(());
    }
    
    for domain in &domains {
        let output = Command::new("launchctl")
            .args(["start", domain])
            .output()?;
        
        if output.status.success() {
            println!("✓ Started: {}", domain);
        } else {
            eprintln!("⚠ Failed to start {}: {}", domain, 
                String::from_utf8_lossy(&output.stderr));
        }
    }

    Ok(())
}

async fn stop_service() -> Result<()> {
    let domains = get_installed_domains()?;
    
    if domains.is_empty() {
        println!("No services installed");
        return Ok(());
    }
    
    for domain in &domains {
        let output = Command::new("launchctl")
            .args(["stop", domain])
            .output()?;
        
        if output.status.success() {
            println!("✓ Stopped: {}", domain);
        } else {
            eprintln!("⚠ Failed to stop {}: {}", domain, 
                String::from_utf8_lossy(&output.stderr));
        }
    }

    Ok(())
}

async fn status_service() -> Result<()> {
    let domains = get_installed_domains()?;
    
    if domains.is_empty() {
        println!("No services installed");
        return Ok(());
    }
    
    for domain in &domains {
        let output = Command::new("launchctl")
            .args(["list", domain])
            .output()?;
        
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            println!("✓ {} is running", domain);
            println!("  {}", stdout.lines().next().unwrap_or(""));
        } else {
            println!("✗ {} is not running", domain);
        }
    }

    Ok(())
}

async fn restart_service() -> Result<()> {
    let domains = get_installed_domains()?;
    
    if domains.is_empty() {
        println!("No services installed");
        return Ok(());
    }
    
    for domain in &domains {
        // Stop first
        let stop_output = Command::new("launchctl")
            .args(["stop", domain])
            .output()?;
        
        if !stop_output.status.success() {
            eprintln!("⚠ Failed to stop {}: {}", domain, 
                String::from_utf8_lossy(&stop_output.stderr));
            continue;
        }
        
        // Small delay to ensure cleanup
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        
        // Then start
        let start_output = Command::new("launchctl")
            .args(["start", domain])
            .output()?;
        
        if start_output.status.success() {
            println!("✓ Restarted: {}", domain);
        } else {
            eprintln!("⚠ Failed to start {}: {}", domain, 
                String::from_utf8_lossy(&start_output.stderr));
        }
    }

    Ok(())
}

async fn uninstall_service(args: &UninstallArgs) -> Result<()> {
    let plist_dir = dirs::home_dir()
        .expect("Home directory not found")
        .join("Library/LaunchAgents");
    
    let domains_to_uninstall = if let Some(domain) = &args.domain {
        vec![domain.clone()]
    } else {
        get_installed_domains()?
    };
    
    if domains_to_uninstall.is_empty() {
        println!("No services to uninstall");
        return Ok(());
    }
    
    for domain in &domains_to_uninstall {
        let plist_path = plist_dir.join(format!("{}.plist", domain));
        
        if !plist_path.exists() {
            println!("⚠ Plist not found: {}", plist_path.display());
            continue;
        }
        
        // Stop the service first
        let _ = Command::new("launchctl")
            .args(["stop", domain])
            .output();
        
        // Unload from launchd
        let unload_output = Command::new("launchctl")
            .args(["unload", "-w", &plist_path.to_string_lossy()])
            .output()?;
        
        if !unload_output.status.success() {
            eprintln!("⚠ Failed to unload {}: {}", domain, 
                String::from_utf8_lossy(&unload_output.stderr));
        }
        
        // Remove plist file
        match fs::remove_file(&plist_path) {
            Ok(_) => {
                println!("✓ Uninstalled: {}", domain);
                println!("  Removed: {}", plist_path.display());
            }
            Err(e) => {
                eprintln!("⚠ Failed to remove plist for {}: {}", domain, e);
            }
        }
    }

    Ok(())
}

/// Get list of installed tgcli service domains
fn get_installed_domains() -> Result<Vec<String>> {
    let plist_dir = dirs::home_dir()
        .expect("Home directory not found")
        .join("Library/LaunchAgents");
    
    let mut domains = Vec::new();
    
    if let Ok(entries) = fs::read_dir(&plist_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "plist") {
                if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                    if filename.starts_with("com.tgcli.") && filename.ends_with(".plist") {
                        let domain = filename.trim_end_matches(".plist");
                        domains.push(domain.to_string());
                    }
                }
            }
        }
    }
    
    Ok(domains)
}
