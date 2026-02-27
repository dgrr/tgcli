use anyhow::Result;
use crate::store::Store;

#[derive(Debug, Clone)]
pub struct WebhookConfig {
    pub url: String,
    pub prompt: String,
    pub chat_id: Option<i64>,
}

impl Store {
    pub async fn set_webhook(&self, url: &str, prompt: &str, chat_id: Option<i64>) -> Result<()> {
        self.ensure_webhook_table().await?;
        self.conn.execute("DELETE FROM webhook_config", ()).await?;
        self.conn
            .execute(
                "INSERT INTO webhook_config (url, prompt, chat_id) VALUES (?1, ?2, ?3)",
                (url, prompt, chat_id),
            )
            .await?;
        Ok(())
    }

    pub async fn get_webhook(&self) -> Result<Option<WebhookConfig>> {
        self.ensure_webhook_table().await?;
        let mut rows = self
            .conn
            .query("SELECT url, prompt, chat_id FROM webhook_config LIMIT 1", ())
            .await?;
        if let Some(row) = rows.next().await? {
            Ok(Some(WebhookConfig {
                url: row.get(0)?,
                prompt: row.get(1)?,
                chat_id: row.get::<Option<i64>>(2)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn remove_webhook(&self) -> Result<bool> {
        self.ensure_webhook_table().await?;
        let affected = self.conn.execute("DELETE FROM webhook_config", ()).await?;
        Ok(affected > 0)
    }

    async fn ensure_webhook_table(&self) -> Result<()> {
        self.conn
            .execute(
                "CREATE TABLE IF NOT EXISTS webhook_config (
                    id INTEGER PRIMARY KEY DEFAULT 1,
                    url TEXT NOT NULL,
                    prompt TEXT NOT NULL,
                    chat_id INTEGER
                )",
                (),
            )
            .await?;
        Ok(())
    }
}

/// Fire a webhook for an incoming message. Non-blocking (spawns a task).
pub fn fire_webhook(config: &WebhookConfig, chat_id: i64, sender_id: i64, text: &str) {
    // Check chat filter
    if let Some(filter_id) = config.chat_id {
        if chat_id != filter_id {
            return;
        }
    }

    // Skip empty messages
    if text.is_empty() {
        return;
    }

    let url = config.url.clone();
    let body = format!(
        "Prompt: {}\n[from:{}] {}: {}",
        config.prompt, chat_id, sender_id, text
    );

    tokio::spawn(async move {
        let client = reqwest::Client::new();
        match client
            .post(&url)
            .header("Content-Type", "text/plain")
            .body(body)
            .send()
            .await
        {
            Ok(resp) => {
                if !resp.status().is_success() {
                    log::warn!("Webhook returned status {}", resp.status());
                }
            }
            Err(e) => {
                log::warn!("Webhook request failed: {}", e);
            }
        }
    });
}
