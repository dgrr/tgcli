pub mod send;
pub mod socket;
pub mod sync;

use crate::store::Store;
use crate::tg::TgClient;
use crate::Cli;
use anyhow::{Context, Result};
use grammers_session::defs::PeerRef;
use grammers_session::updates::UpdatesLike;
use grammers_tl_types as tl;
use tokio::sync::mpsc;

pub struct App {
    pub tg: TgClient,
    pub store: Store,
    pub store_dir: String,
    #[allow(dead_code)]
    pub json: bool,
    pub updates_rx: Option<mpsc::UnboundedReceiver<UpdatesLike>>,
}

impl App {
    pub async fn new(cli: &Cli) -> Result<Self> {
        let store_dir = cli.store_dir();
        std::fs::create_dir_all(&store_dir)
            .with_context(|| format!("Failed to create store directory '{}'", store_dir))?;

        let session_path = format!("{}/session.db", store_dir);
        // SqliteSession::open creates the file if it doesn't exist

        let (tg, updates_rx) = TgClient::connect_with_updates(&session_path)
            .context("Failed to connect to Telegram")?;

        if !tg
            .client
            .is_authorized()
            .await
            .context("Failed to check authorization status")?
        {
            anyhow::bail!("Session expired or not authenticated. Run `tgcli auth` first.");
        }

        let store = Store::open(&store_dir)
            .await
            .context("Failed to open message store database")?;

        Ok(App {
            tg,
            store,
            store_dir,
            json: cli.json,
            updates_rx: Some(updates_rx),
        })
    }

    /// Create App without requiring authorization (for auth command).
    pub async fn new_unauthed(cli: &Cli) -> Result<Self> {
        let store_dir = cli.store_dir();
        std::fs::create_dir_all(&store_dir)
            .with_context(|| format!("Failed to create store directory '{}'", store_dir))?;

        let session_path = format!("{}/session.db", store_dir);

        let (tg, updates_rx) = TgClient::connect_with_updates(&session_path)
            .context("Failed to connect to Telegram")?;
        let store = Store::open(&store_dir)
            .await
            .context("Failed to open message store database")?;

        Ok(App {
            tg,
            store,
            store_dir,
            json: cli.json,
            updates_rx: Some(updates_rx),
        })
    }

    /// Sync forum topics from Telegram for a given chat.
    /// Returns the number of topics synced.
    pub async fn sync_topics(&self, chat_id: i64) -> Result<usize> {
        // Resolve peer via dialogs
        let peer_ref = self.resolve_peer_ref_for_topics(chat_id).await?;

        // Convert to InputPeer for the API call
        let input_peer: tl::enums::InputPeer = peer_ref.into();

        // Fetch topics using raw TL function
        let request = tl::functions::messages::GetForumTopics {
            peer: input_peer,
            q: None,
            offset_date: 0,
            offset_id: 0,
            offset_topic: 0,
            limit: 100,
        };

        let result = self
            .tg
            .client
            .invoke(&request)
            .await
            .with_context(|| format!("Failed to fetch forum topics for chat {}", chat_id))?;

        let topics = match result {
            tl::enums::messages::ForumTopics::Topics(t) => t.topics,
        };

        let mut count = 0;
        for topic_enum in topics {
            match topic_enum {
                tl::enums::ForumTopic::Topic(topic) => {
                    // Convert icon_emoji_id to string representation if present
                    let icon_emoji = topic.icon_emoji_id.map(|id| id.to_string());

                    self.store
                        .upsert_topic(
                            chat_id,
                            topic.id,
                            &topic.title,
                            topic.icon_color,
                            icon_emoji.as_deref(),
                        )
                        .await?;
                    count += 1;
                }
                tl::enums::ForumTopic::Deleted(_) => {
                    // Skip deleted topics
                }
            }
        }

        Ok(count)
    }

    /// Resolve a chat ID to a PeerRef for topics API.
    async fn resolve_peer_ref_for_topics(&self, chat_id: i64) -> Result<PeerRef> {
        let mut dialogs = self.tg.client.iter_dialogs();
        while let Some(dialog) = dialogs.next().await.with_context(|| {
            format!("Failed to iterate dialogs while resolving chat {}", chat_id)
        })? {
            let peer = dialog.peer();
            if peer.id().bare_id() == chat_id {
                return Ok(PeerRef::from(peer));
            }
        }
        anyhow::bail!(
            "Chat {} not found. Run `tgcli sync` to refresh your chat list.",
            chat_id
        );
    }

    /// Mark all forum topics in a chat as read.
    /// Returns the number of topics marked as read.
    pub async fn mark_read_all_topics(&self, chat_id: i64) -> Result<usize> {
        let peer_ref = self.resolve_peer_ref_for_topics(chat_id).await?;
        let input_peer: tl::enums::InputPeer = peer_ref.into();

        // First, fetch all topics
        let request = tl::functions::messages::GetForumTopics {
            peer: input_peer.clone(),
            q: None,
            offset_date: 0,
            offset_id: 0,
            offset_topic: 0,
            limit: 100,
        };

        let result = self
            .tg
            .client
            .invoke(&request)
            .await
            .with_context(|| format!("Failed to fetch forum topics for chat {}", chat_id))?;

        let topics = match result {
            tl::enums::messages::ForumTopics::Topics(t) => t.topics,
        };

        let mut count = 0;
        for topic_enum in topics {
            if let tl::enums::ForumTopic::Topic(topic) = topic_enum {
                // Mark this topic as read
                let read_request = tl::functions::messages::ReadDiscussion {
                    peer: input_peer.clone(),
                    msg_id: topic.id,
                    read_max_id: topic.top_message,
                };

                match self.tg.client.invoke(&read_request).await {
                    Ok(_) => {
                        count += 1;
                    }
                    Err(e) => {
                        log::warn!("Failed to mark topic {} as read: {}", topic.id, e);
                    }
                }
            }
        }

        Ok(count)
    }
}
