pub mod send;
pub mod socket;
pub mod sync;

use crate::store::Store;
use crate::tg::TgClient;
use crate::Cli;
use anyhow::Result;
use grammers_session::updates::UpdatesLike;
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
        std::fs::create_dir_all(&store_dir)?;

        let session_path = format!("{}/session.db", store_dir);
        // SqliteSession::open creates the file if it doesn't exist

        let (tg, updates_rx) = TgClient::connect_with_updates(&session_path)?;

        if !tg.client.is_authorized().await? {
            anyhow::bail!("Session expired or not authenticated. Run `tgrs auth` first.");
        }

        let store = Store::open(&store_dir)?;

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
        std::fs::create_dir_all(&store_dir)?;

        let session_path = format!("{}/session.db", store_dir);

        let (tg, updates_rx) = TgClient::connect_with_updates(&session_path)?;
        let store = Store::open(&store_dir)?;

        Ok(App {
            tg,
            store,
            store_dir,
            json: cli.json,
            updates_rx: Some(updates_rx),
        })
    }
}
