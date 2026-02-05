use crate::app::App;
use crate::out;
use crate::Cli;
use anyhow::{Context, Result};
use clap::Subcommand;
use grammers_tl_types as tl;
use serde::Serialize;

#[derive(Subcommand, Debug, Clone)]
pub enum ProfileCommand {
    /// Show your own profile info
    Show,
    /// Update your profile
    Set {
        /// Set your first name
        #[arg(long)]
        first_name: Option<String>,
        /// Set your last name
        #[arg(long)]
        last_name: Option<String>,
        /// Set your bio/about text
        #[arg(long)]
        bio: Option<String>,
        /// Set your username (without @)
        #[arg(long)]
        username: Option<String>,
    },
}

#[derive(Serialize)]
struct ProfileInfo {
    id: i64,
    first_name: String,
    last_name: Option<String>,
    username: Option<String>,
    phone: Option<String>,
    bio: Option<String>,
    premium: bool,
}

pub async fn run(cli: &Cli, cmd: &ProfileCommand) -> Result<()> {
    let app = App::new(cli).await?;

    match cmd {
        ProfileCommand::Show => {
            // Get self user info
            let me = app
                .tg
                .client
                .get_me()
                .await
                .context("Failed to get profile info")?;

            // Get full user info for bio
            let input_user = tl::enums::InputUser::UserSelf;
            let request = tl::functions::users::GetFullUser { id: input_user };
            let full_user = app
                .tg
                .client
                .invoke(&request)
                .await
                .context("Failed to get full profile info")?;

            let bio = match full_user {
                tl::enums::users::UserFull::Full(f) => match f.full_user {
                    tl::enums::UserFull::Full(uf) => uf.about,
                },
            };

            // Check if premium by looking at raw user data
            let is_premium = match &me.raw {
                tl::enums::User::User(u) => u.premium,
                _ => false,
            };

            let profile = ProfileInfo {
                id: me.bare_id(),
                first_name: me.first_name().unwrap_or("").to_string(),
                last_name: me.last_name().map(|s| s.to_string()),
                username: me.username().map(|s| s.to_string()),
                phone: me.phone().map(|s| s.to_string()),
                bio,
                premium: is_premium,
            };

            if cli.json {
                out::write_json(&profile)?;
            } else {
                println!("ID: {}", profile.id);
                println!("Name: {}", profile.first_name);
                if let Some(ref last) = profile.last_name {
                    println!("Last name: {}", last);
                }
                if let Some(ref username) = profile.username {
                    println!("Username: @{}", username);
                }
                if let Some(ref phone) = profile.phone {
                    println!("Phone: {}", phone);
                }
                if let Some(ref bio) = profile.bio {
                    println!("Bio: {}", bio);
                }
                if profile.premium {
                    println!("Premium: yes");
                }
            }
        }
        ProfileCommand::Set {
            first_name,
            last_name,
            bio,
            username,
        } => {
            let mut updated = Vec::new();

            // Update name if provided
            if first_name.is_some() || last_name.is_some() {
                // Get current info first
                let me = app
                    .tg
                    .client
                    .get_me()
                    .await
                    .context("Failed to get current profile")?;

                let new_first = first_name
                    .clone()
                    .unwrap_or_else(|| me.first_name().unwrap_or("").to_string());
                let new_last = if last_name.is_some() {
                    last_name.clone().unwrap_or_default()
                } else {
                    me.last_name().unwrap_or("").to_string()
                };

                let request = tl::functions::account::UpdateProfile {
                    first_name: Some(new_first.clone()),
                    last_name: Some(new_last.clone()),
                    about: None,
                };

                app.tg
                    .client
                    .invoke(&request)
                    .await
                    .context("Failed to update name")?;

                if first_name.is_some() {
                    updated.push(format!("first_name: {}", new_first));
                }
                if last_name.is_some() {
                    updated.push(format!("last_name: {}", new_last));
                }
            }

            // Update bio if provided
            if let Some(ref new_bio) = bio {
                let request = tl::functions::account::UpdateProfile {
                    first_name: None,
                    last_name: None,
                    about: Some(new_bio.clone()),
                };

                app.tg
                    .client
                    .invoke(&request)
                    .await
                    .context("Failed to update bio")?;

                updated.push(format!("bio: {}", new_bio));
            }

            // Update username if provided
            if let Some(ref new_username) = username {
                let request = tl::functions::account::UpdateUsername {
                    username: new_username.clone(),
                };

                app.tg
                    .client
                    .invoke(&request)
                    .await
                    .context("Failed to update username. It may already be taken or invalid.")?;

                updated.push(format!("username: @{}", new_username));
            }

            if updated.is_empty() {
                anyhow::bail!(
                    "No changes specified. Use --first-name, --last-name, --bio, or --username."
                );
            }

            if cli.json {
                out::write_json(&serde_json::json!({
                    "updated": true,
                    "changes": updated,
                }))?;
            } else {
                println!("Profile updated:");
                for change in &updated {
                    println!("  - {}", change);
                }
            }
        }
    }
    Ok(())
}
