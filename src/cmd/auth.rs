use crate::app::App;
use crate::out;
use crate::tg;
use crate::Cli;
use anyhow::{Context, Result};
use clap::Args;
use std::io::{self, Write};

#[derive(Args, Debug, Clone)]
pub struct AuthArgs {
    #[command(subcommand)]
    pub cmd: Option<AuthCommand>,
}

#[derive(clap::Subcommand, Debug, Clone)]
pub enum AuthCommand {
    /// Check authentication status
    Status,
    /// Remove session / logout
    Logout,
}

pub async fn run(cli: &Cli, args: &AuthArgs) -> Result<()> {
    match &args.cmd {
        Some(AuthCommand::Status) => status(cli).await,
        Some(AuthCommand::Logout) => logout(cli).await,
        None => interactive_auth(cli).await,
    }
}

async fn interactive_auth(cli: &Cli) -> Result<()> {
    let app = App::new_unauthed(cli).await?;
    let client = &app.tg.client;

    eprintln!("Starting Telegram authentication…");

    // Get phone number
    eprint!("Phone number (international format, e.g. +34612345678): ");
    io::stderr().flush()?;
    let mut phone = String::new();
    io::stdin().read_line(&mut phone)?;
    let phone = phone.trim().to_string();

    if phone.is_empty() {
        anyhow::bail!("Phone number is required");
    }

    // Request login code
    let token = client
        .request_login_code(&phone, tg::API_HASH)
        .await
        .with_context(|| format!("Failed to request login code for {}", phone))?;
    eprintln!("Login code sent via Telegram.");

    eprint!("Enter the code: ");
    io::stderr().flush()?;
    let mut code = String::new();
    io::stdin().read_line(&mut code)?;
    let code = code.trim().to_string();

    // Sign in
    use grammers_client::SignInError;
    match client.sign_in(&token, &code).await {
        Ok(user) => {
            let name = user.first_name().map(|s| s.to_string()).unwrap_or_default();
            if cli.output.is_json() {
                out::write_json(&serde_json::json!({
                    "authenticated": true,
                    "user": name,
                }))?;
            } else {
                eprintln!("Authenticated as {}.", name);
            }
        }
        Err(SignInError::PasswordRequired(password_token)) => {
            eprintln!("Two-factor authentication required.");
            let hint = password_token
                .hint()
                .map(|s| s.to_string())
                .unwrap_or_default();
            if !hint.is_empty() {
                eprintln!("Password hint: {}", hint);
            }
            let password = rpassword::prompt_password("Enter 2FA password: ")?;
            let user = client
                .check_password(password_token, password.as_bytes().to_vec())
                .await
                .context("Failed to verify 2FA password")?;
            let name = user.first_name().map(|s| s.to_string()).unwrap_or_default();
            if cli.output.is_json() {
                out::write_json(&serde_json::json!({
                    "authenticated": true,
                    "user": name,
                }))?;
            } else {
                eprintln!("Authenticated as {}.", name);
            }
        }
        Err(e) => {
            anyhow::bail!("Sign in failed: {}", e);
        }
    }

    Ok(())
}

async fn status(cli: &Cli) -> Result<()> {
    let store_dir = cli.store_dir();
    let session_path = format!("{}/session.db", store_dir);

    if !std::path::Path::new(&session_path).exists() {
        if cli.output.is_json() {
            out::write_json(&serde_json::json!({
                "authenticated": false,
            }))?;
        } else {
            println!("Not authenticated. Run `tgcli auth`.");
        }
        return Ok(());
    }

    match App::new_unauthed(cli).await {
        Ok(app) => {
            let authed = app.tg.client.is_authorized().await?;
            if cli.output.is_json() {
                out::write_json(&serde_json::json!({
                    "authenticated": authed,
                }))?;
            } else if authed {
                println!("Authenticated.");
            } else {
                println!("Session exists but not authenticated. Run `tgcli auth`.");
            }
        }
        Err(_) => {
            if cli.output.is_json() {
                out::write_json(&serde_json::json!({
                    "authenticated": false,
                    "error": "Failed to connect",
                }))?;
            } else {
                println!("Session exists but failed to connect. Try `tgcli auth`.");
            }
        }
    }

    Ok(())
}

async fn logout(cli: &Cli) -> Result<()> {
    let store_dir = cli.store_dir();
    let session_path = format!("{}/session.db", store_dir);

    if !std::path::Path::new(&session_path).exists() {
        anyhow::bail!("No session found. Nothing to logout from.");
    }

    let app = App::new_unauthed(cli).await?;
    app.tg
        .client
        .sign_out()
        .await
        .context("Failed to sign out from Telegram")?;
    // Remove session file
    let _ = std::fs::remove_file(&session_path);

    if cli.output.is_json() {
        out::write_json(&serde_json::json!({ "logged_out": true }))?;
    } else {
        println!("Logged out.");
    }
    Ok(())
}
