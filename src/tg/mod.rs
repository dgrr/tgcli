use anyhow::Result;
use grammers_client::Client;
use grammers_mtsender::SenderPool;
use grammers_session::storages::SqliteSession;
use grammers_session::updates::UpdatesLike;
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
    pub fn connect_with_updates(
        session_path: &str,
    ) -> Result<(Self, mpsc::UnboundedReceiver<UpdatesLike>)> {
        let session = Arc::new(
            SqliteSession::open(session_path)
                .map_err(|e| anyhow::anyhow!("Failed to open session: {}", e))?,
        );

        let pool = SenderPool::new(Arc::clone(&session) as Arc<SqliteSession>, API_ID);
        let client = Client::new(&pool);

        let SenderPool {
            runner,
            updates,
            ..
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

impl Drop for TgClient {
    fn drop(&mut self) {
        self.client.disconnect();
        self.pool_handle.abort();
    }
}
