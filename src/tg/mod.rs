use anyhow::{Context, Result};
use grammers_client::peer::{Group, Peer, User};
use grammers_client::Client;
use grammers_mtsender::SenderPool;
use grammers_session::storages::SqliteSession;
use grammers_session::types::{PeerAuth, PeerRef};
use grammers_session::updates::UpdatesLike;
use grammers_tl_types as tl;
use std::sync::Arc;
use tokio::sync::mpsc;

pub const API_ID: i32 = 32529142;
pub const API_HASH: &str = "cf7543485b4c077f67423f57fe42911f";

/// A connected Telegram client with its pool runner handle.
pub struct TgClient {
    pub client: Client,
    #[allow(dead_code)]
    pub session: Arc<SqliteSession>,
    pool_handle: tokio::task::JoinHandle<()>,
}

impl TgClient {
    /// Connect with updates support.
    /// Returns the client and an updates receiver.
    pub async fn connect_with_updates(
        session_path: &str,
    ) -> Result<(Self, mpsc::UnboundedReceiver<UpdatesLike>)> {
        let session = Arc::new(SqliteSession::open(session_path).await.with_context(|| {
            format!(
                "Failed to open session database at '{}'. Check file permissions.",
                session_path
            )
        })?);

        let pool = SenderPool::new(Arc::clone(&session) as Arc<SqliteSession>, API_ID);
        let client = Client::new(pool.handle);

        let SenderPool {
            runner, updates, ..
        } = pool;

        let pool_handle = tokio::spawn(async move {
            runner.run().await;
        });

        Ok((
            TgClient {
                client,
                session,
                pool_handle,
            },
            updates,
        ))
    }
}

/// Resolve a [`Peer`] to a [`PeerRef`].
///
/// Prefers grammers' own resolution ([`Peer::to_ref`]), which uses the peer's
/// non-`min` auth when it has one and otherwise consults the session cache.
/// When neither is available, fall back to whatever access hash the raw peer
/// carries, which is what the pre-0.10 `PeerRef::from(&peer)` did.
pub async fn peer_to_ref_async(peer: &Peer) -> Result<PeerRef> {
    let cached = peer
        .to_ref()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to look up peer in the session cache: {e}"))?;

    Ok(cached.unwrap_or_else(|| peer_to_ref(peer)))
}

/// Build a [`PeerRef`] from the access hash carried by the raw peer object,
/// falling back to ambient auth.
///
/// Only used when [`Peer::to_ref`] cannot produce a reference; this replicates
/// the pre-0.10 `PeerRef::from(&peer)` behavior.
fn peer_to_ref(peer: &Peer) -> PeerRef {
    let auth = match peer {
        Peer::User(user) => user_auth(user),
        Peer::Channel(channel) => channel_auth(&channel.raw),
        Peer::Group(group) => group_auth(group),
    };
    PeerRef {
        id: peer.id(),
        auth,
    }
}

fn user_auth(user: &User) -> PeerAuth {
    match &user.raw {
        tl::enums::User::User(inner) => inner
            .access_hash
            .map(PeerAuth::from_hash)
            .unwrap_or_default(),
        _ => PeerAuth::default(),
    }
}

/// A [`Group`] wraps `tl::enums::Chat`, which is either a basic group (addressed
/// by ID alone) or a megagroup/gigagroup channel that does need an access hash.
fn group_auth(group: &Group) -> PeerAuth {
    match &group.raw {
        tl::enums::Chat::Channel(channel) => channel_auth(channel),
        tl::enums::Chat::ChannelForbidden(channel) => PeerAuth::from_hash(channel.access_hash),
        _ => PeerAuth::default(),
    }
}

fn channel_auth(channel: &tl::types::Channel) -> PeerAuth {
    channel
        .access_hash
        .map(PeerAuth::from_hash)
        .unwrap_or_default()
}

impl Drop for TgClient {
    fn drop(&mut self) {
        self.client.disconnect();
        self.pool_handle.abort();
    }
}
