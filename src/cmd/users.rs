use crate::app::App;
use crate::out;
use crate::out::markdown::{ToMarkdown, UserInfoMd};
use crate::Cli;
use anyhow::{Context, Result};
use clap::Subcommand;
use grammers_session::defs::{PeerId, PeerRef};
use grammers_session::Session;
use grammers_tl_types as tl;
use serde::Serialize;

#[derive(Subcommand, Debug, Clone)]
pub enum UsersCommand {
    /// Show user info
    Show {
        /// User ID
        #[arg(long)]
        id: i64,
    },
    /// Block a user
    Block {
        /// User ID
        #[arg(long)]
        id: i64,
    },
    /// Unblock a user
    Unblock {
        /// User ID
        #[arg(long)]
        id: i64,
    },
}

#[derive(Serialize)]
struct UserInfo {
    id: i64,
    first_name: Option<String>,
    last_name: Option<String>,
    username: Option<String>,
    phone: Option<String>,
    bio: Option<String>,
    is_bot: bool,
    is_verified: bool,
    is_premium: bool,
    is_scam: bool,
    is_fake: bool,
    is_blocked: bool,
    common_chats_count: i32,
}

pub async fn run(cli: &Cli, cmd: &UsersCommand) -> Result<()> {
    match cmd {
        UsersCommand::Show { id } => show_user(cli, *id).await,
        UsersCommand::Block { id } => block_user(cli, *id, true).await,
        UsersCommand::Unblock { id } => block_user(cli, *id, false).await,
    }
}

async fn show_user(cli: &Cli, user_id: i64) -> Result<()> {
    let app = App::new(cli).await?;

    // Resolve user_id to InputUser
    let input_user = resolve_user_to_input_user(&app, user_id).await?;

    // Get full user info
    let request = tl::functions::users::GetFullUser { id: input_user };

    let result = app
        .tg
        .client
        .invoke(&request)
        .await
        .with_context(|| format!("Failed to get user info for {}", user_id))?;

    // Extract full_user and users from the response
    let tl::types::users::UserFull {
        full_user, users, ..
    } = result.into();

    // Get the UserFull data
    let tl::enums::UserFull::Full(full) = full_user;

    // Find the user in the users list
    let user = users.iter().find_map(|u| match u {
        tl::enums::User::User(user) if user.id == user_id => Some(user),
        _ => None,
    });

    let info = UserInfo {
        id: full.id,
        first_name: user.and_then(|u| u.first_name.clone()),
        last_name: user.and_then(|u| u.last_name.clone()),
        username: user.and_then(|u| u.username.clone()),
        phone: user.and_then(|u| u.phone.clone()),
        bio: full.about.clone(),
        is_bot: user.map(|u| u.bot).unwrap_or(false),
        is_verified: user.map(|u| u.verified).unwrap_or(false),
        is_premium: user.map(|u| u.premium).unwrap_or(false),
        is_scam: user.map(|u| u.scam).unwrap_or(false),
        is_fake: user.map(|u| u.fake).unwrap_or(false),
        is_blocked: full.blocked,
        common_chats_count: full.common_chats_count,
    };

    if cli.output.is_json() {
        out::write_json(&info)?;
    } else if cli.output.is_markdown() {
        let info_md = UserInfoMd {
            id: info.id,
            first_name: info.first_name.clone(),
            last_name: info.last_name.clone(),
            username: info.username.clone(),
            phone: info.phone.clone(),
            bio: info.bio.clone(),
            is_bot: info.is_bot,
            is_verified: info.is_verified,
            is_premium: info.is_premium,
            is_scam: info.is_scam,
            is_fake: info.is_fake,
            is_blocked: info.is_blocked,
            common_chats_count: info.common_chats_count,
        };
        out::write_markdown(&info_md.to_markdown());
    } else {
        println!("ID: {}", info.id);

        let name = match (&info.first_name, &info.last_name) {
            (Some(f), Some(l)) => format!("{} {}", f, l),
            (Some(f), None) => f.clone(),
            (None, Some(l)) => l.clone(),
            (None, None) => "(no name)".to_string(),
        };
        println!("Name: {}", name);

        if let Some(u) = &info.username {
            println!("Username: @{}", u);
        }
        if let Some(p) = &info.phone {
            println!("Phone: +{}", p);
        }
        if let Some(b) = &info.bio {
            println!("Bio: {}", b);
        }

        // Status flags
        let mut flags = Vec::new();
        if info.is_bot {
            flags.push("bot");
        }
        if info.is_verified {
            flags.push("verified");
        }
        if info.is_premium {
            flags.push("premium");
        }
        if info.is_scam {
            flags.push("scam");
        }
        if info.is_fake {
            flags.push("fake");
        }
        if info.is_blocked {
            flags.push("blocked");
        }
        if !flags.is_empty() {
            println!("Flags: {}", flags.join(", "));
        }

        if info.common_chats_count > 0 {
            println!("Common chats: {}", info.common_chats_count);
        }
    }

    Ok(())
}

async fn block_user(cli: &Cli, user_id: i64, block: bool) -> Result<()> {
    let app = App::new(cli).await?;

    // Resolve user_id to InputPeer (block/unblock use InputPeer, not InputUser)
    let input_peer = resolve_user_to_input_peer(&app, user_id).await?;

    if block {
        let request = tl::functions::contacts::Block {
            my_stories_from: false,
            id: input_peer,
        };

        app.tg
            .client
            .invoke(&request)
            .await
            .with_context(|| format!("Failed to block user {}", user_id))?;
    } else {
        let request = tl::functions::contacts::Unblock {
            my_stories_from: false,
            id: input_peer,
        };

        app.tg
            .client
            .invoke(&request)
            .await
            .with_context(|| format!("Failed to unblock user {}", user_id))?;
    }

    let action = if block { "Blocked" } else { "Unblocked" };

    if cli.output.is_json() {
        out::write_json(&serde_json::json!({
            "success": true,
            "user_id": user_id,
            "action": action.to_lowercase(),
        }))?;
    } else {
        println!("{} user {}", action, user_id);
    }

    Ok(())
}

/// Resolve a user ID to an InputUser for API calls.
async fn resolve_user_to_input_user(app: &App, user_id: i64) -> Result<tl::enums::InputUser> {
    // First check session for the user's access_hash
    let user_peer_id = PeerId::user(user_id);
    if let Some(info) = app.tg.session.peer(user_peer_id) {
        let peer_ref = PeerRef {
            id: user_peer_id,
            auth: info.auth(),
        };
        // PeerRef has From<PeerRef> for tl::enums::InputUser
        return Ok(peer_ref.into());
    }

    // Try to find user in dialogs
    let mut dialogs = app.tg.client.iter_dialogs();
    while let Some(dialog) = dialogs.next().await? {
        let peer = dialog.peer();
        if peer.id().bare_id() == user_id {
            let peer_ref = PeerRef::from(peer);
            return Ok(peer_ref.into());
        }
    }

    anyhow::bail!(
        "Could not resolve user {}. Make sure you have a chat with them or they're in your contacts.",
        user_id
    );
}

/// Resolve a user ID to an InputPeer for API calls.
async fn resolve_user_to_input_peer(app: &App, user_id: i64) -> Result<tl::enums::InputPeer> {
    // First check session for the user's access_hash
    let user_peer_id = PeerId::user(user_id);
    if let Some(info) = app.tg.session.peer(user_peer_id) {
        let peer_ref = PeerRef {
            id: user_peer_id,
            auth: info.auth(),
        };
        return Ok(peer_ref.into());
    }

    // Try to find user in dialogs
    let mut dialogs = app.tg.client.iter_dialogs();
    while let Some(dialog) = dialogs.next().await? {
        let peer = dialog.peer();
        if peer.id().bare_id() == user_id {
            return Ok(PeerRef::from(peer).into());
        }
    }

    anyhow::bail!(
        "Could not resolve user {}. Make sure you have a chat with them or they're in your contacts.",
        user_id
    );
}
