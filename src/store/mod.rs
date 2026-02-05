use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::path::Path;
use turso::{Builder, Connection, Database, Row};

pub struct Store {
    conn: Connection,
    has_fts: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Chat {
    pub id: i64,
    pub kind: String,
    pub name: String,
    pub username: Option<String>,
    pub last_message_ts: Option<DateTime<Utc>>,
    #[serde(default)]
    pub is_forum: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Topic {
    pub chat_id: i64,
    pub topic_id: i32,
    pub name: String,
    pub icon_color: i32,
    pub icon_emoji: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Contact {
    pub user_id: i64,
    pub username: Option<String>,
    pub first_name: String,
    pub last_name: String,
    pub phone: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub id: i64,
    pub chat_id: i64,
    pub sender_id: i64,
    pub ts: DateTime<Utc>,
    pub edit_ts: Option<DateTime<Utc>>,
    pub from_me: bool,
    pub text: String,
    pub media_type: Option<String>,
    pub media_path: Option<String>,
    pub reply_to_id: Option<i64>,
    pub topic_id: Option<i32>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub snippet: String,
}

pub struct ListMessagesParams {
    pub chat_id: Option<i64>,
    pub topic_id: Option<i32>,
    pub limit: i64,
    pub after: Option<DateTime<Utc>>,
    pub before: Option<DateTime<Utc>>,
    pub ignore_chats: Vec<i64>,
    pub ignore_channels: bool,
}

pub struct SearchMessagesParams {
    pub query: String,
    pub chat_id: Option<i64>,
    pub from_id: Option<i64>,
    pub limit: i64,
    pub media_type: Option<String>,
    pub ignore_chats: Vec<i64>,
    pub ignore_channels: bool,
}

pub struct UpsertMessageParams {
    pub id: i64,
    pub chat_id: i64,
    pub sender_id: i64,
    pub ts: DateTime<Utc>,
    pub edit_ts: Option<DateTime<Utc>>,
    pub from_me: bool,
    pub text: String,
    pub media_type: Option<String>,
    pub media_path: Option<String>,
    pub reply_to_id: Option<i64>,
    pub topic_id: Option<i32>,
}

impl Store {
    pub async fn open(store_dir: &str) -> Result<Self> {
        std::fs::create_dir_all(store_dir)?;
        let db_path = Path::new(store_dir).join("tgcli.db");
        let db_path_str = db_path.to_string_lossy();
        let db: Database = Builder::new_local(&db_path_str)
            .build()
            .await
            .context("Failed to open database")?;
        let conn = db.connect().context("Failed to connect to database")?;

        // PRAGMAs that set values return the new value, so use query and ignore results
        let _ = conn.query("PRAGMA journal_mode=WAL", ()).await;
        let _ = conn.query("PRAGMA busy_timeout=5000", ()).await;

        let mut store = Store {
            conn,
            has_fts: false,
        };
        store.migrate().await?;
        Ok(store)
    }

    async fn migrate(&mut self) -> Result<()> {
        // Create tables one at a time (turso execute doesn't support multiple statements)
        self.conn
            .execute(
                "CREATE TABLE IF NOT EXISTS chats (
                    id INTEGER PRIMARY KEY,
                    kind TEXT NOT NULL DEFAULT 'user',
                    name TEXT NOT NULL DEFAULT '',
                    username TEXT,
                    last_message_ts TEXT,
                    is_forum INTEGER NOT NULL DEFAULT 0
                )",
                (),
            )
            .await
            .context("Failed to create chats table")?;

        self.conn
            .execute(
                "CREATE TABLE IF NOT EXISTS contacts (
                    user_id INTEGER PRIMARY KEY,
                    username TEXT,
                    first_name TEXT NOT NULL DEFAULT '',
                    last_name TEXT NOT NULL DEFAULT '',
                    phone TEXT NOT NULL DEFAULT ''
                )",
                (),
            )
            .await
            .context("Failed to create contacts table")?;

        self.conn
            .execute(
                "CREATE TABLE IF NOT EXISTS messages (
                    id INTEGER NOT NULL,
                    chat_id INTEGER NOT NULL,
                    sender_id INTEGER NOT NULL DEFAULT 0,
                    ts TEXT NOT NULL,
                    edit_ts TEXT,
                    from_me INTEGER NOT NULL DEFAULT 0,
                    text TEXT NOT NULL DEFAULT '',
                    media_type TEXT,
                    media_path TEXT,
                    reply_to_id INTEGER,
                    topic_id INTEGER,
                    PRIMARY KEY (chat_id, id)
                )",
                (),
            )
            .await
            .context("Failed to create messages table")?;

        // Create topics table for forum groups
        self.conn
            .execute(
                "CREATE TABLE IF NOT EXISTS topics (
                    chat_id INTEGER NOT NULL,
                    topic_id INTEGER NOT NULL,
                    name TEXT NOT NULL DEFAULT '',
                    icon_color INTEGER NOT NULL DEFAULT 0,
                    icon_emoji TEXT,
                    PRIMARY KEY (chat_id, topic_id)
                )",
                (),
            )
            .await
            .context("Failed to create topics table")?;

        // Add media_path column if it doesn't exist (migration for existing DBs)
        let _ = self
            .conn
            .execute("ALTER TABLE messages ADD COLUMN media_path TEXT", ())
            .await;

        // Add is_forum column if it doesn't exist (migration for existing DBs)
        let _ = self
            .conn
            .execute(
                "ALTER TABLE chats ADD COLUMN is_forum INTEGER NOT NULL DEFAULT 0",
                (),
            )
            .await;

        // Add topic_id column if it doesn't exist (migration for existing DBs)
        let _ = self
            .conn
            .execute("ALTER TABLE messages ADD COLUMN topic_id INTEGER", ())
            .await;

        self.conn
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_messages_chat_ts ON messages(chat_id, ts)",
                (),
            )
            .await?;
        self.conn
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_messages_ts ON messages(ts)",
                (),
            )
            .await?;
        self.conn
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_messages_sender ON messages(sender_id)",
                (),
            )
            .await?;

        // Try to create FTS5 table
        let fts_result = self
            .conn
            .execute(
                "CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
                    text,
                    content='messages',
                    content_rowid='rowid'
                )",
                (),
            )
            .await;

        if fts_result.is_err() {
            self.has_fts = false;
            log::warn!("FTS5 not available, search will use LIKE fallback");
            return Ok(());
        }

        // Create triggers for FTS
        let trigger1 = self
            .conn
            .execute(
                "CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
                    INSERT INTO messages_fts(rowid, text) VALUES (new.rowid, new.text);
                END",
                (),
            )
            .await;

        let trigger2 = self
            .conn
            .execute(
                "CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
                    INSERT INTO messages_fts(messages_fts, rowid, text) VALUES('delete', old.rowid, old.text);
                END",
                (),
            )
            .await;

        let trigger3 = self
            .conn
            .execute(
                "CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE ON messages BEGIN
                    INSERT INTO messages_fts(messages_fts, rowid, text) VALUES('delete', old.rowid, old.text);
                    INSERT INTO messages_fts(rowid, text) VALUES (new.rowid, new.text);
                END",
                (),
            )
            .await;

        // All FTS setup succeeded
        self.has_fts = trigger1.is_ok() && trigger2.is_ok() && trigger3.is_ok();
        if !self.has_fts {
            log::warn!("FTS5 triggers failed, search will use LIKE fallback");
            return Ok(());
        }

        // Check if FTS index needs to be populated from existing messages
        // Compare row counts: if messages exist but FTS is empty/underpopulated, rebuild
        let msg_count: i64 = {
            let mut rows = self.conn.query("SELECT COUNT(*) FROM messages", ()).await?;
            rows.next().await?.map(|r| r.get(0).unwrap_or(0)).unwrap_or(0)
        };
        let fts_count: i64 = {
            let mut rows = self.conn.query("SELECT COUNT(*) FROM messages_fts", ()).await?;
            rows.next().await?.map(|r| r.get(0).unwrap_or(0)).unwrap_or(0)
        };

        if msg_count > 0 && fts_count < msg_count {
            log::info!(
                "FTS5 index incomplete ({} vs {} messages), rebuilding...",
                fts_count,
                msg_count
            );
            // Rebuild the entire FTS index from scratch
            let _ = self
                .conn
                .execute("DELETE FROM messages_fts", ())
                .await;
            let rebuild_result = self
                .conn
                .execute(
                    "INSERT INTO messages_fts(rowid, text) SELECT rowid, text FROM messages",
                    (),
                )
                .await;
            if let Err(e) = rebuild_result {
                log::warn!("Failed to populate FTS5 index: {}", e);
            } else {
                log::info!("FTS5 index rebuilt successfully");
            }
        }

        Ok(())
    }

    pub fn has_fts(&self) -> bool {
        self.has_fts
    }

    // --- Chats ---

    pub async fn upsert_chat(
        &self,
        id: i64,
        kind: &str,
        name: &str,
        username: Option<&str>,
        last_message_ts: Option<DateTime<Utc>>,
        is_forum: bool,
    ) -> Result<()> {
        let ts_str = last_message_ts.map(|t| t.to_rfc3339());
        let is_forum_int = is_forum as i64;
        self.conn
            .execute(
                "INSERT INTO chats (id, kind, name, username, last_message_ts, is_forum)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(id) DO UPDATE SET
                    kind = COALESCE(excluded.kind, kind),
                    name = CASE WHEN excluded.name != '' THEN excluded.name ELSE name END,
                    username = COALESCE(excluded.username, username),
                    last_message_ts = CASE WHEN excluded.last_message_ts IS NOT NULL AND (excluded.last_message_ts > last_message_ts OR last_message_ts IS NULL)
                        THEN excluded.last_message_ts ELSE last_message_ts END,
                    is_forum = CASE WHEN excluded.is_forum = 1 THEN 1 ELSE is_forum END",
                (id, kind, name, username, ts_str, is_forum_int),
            )
            .await?;
        Ok(())
    }

    pub async fn list_chats(&self, query: Option<&str>, limit: i64) -> Result<Vec<Chat>> {
        let mut chats = Vec::new();

        if let Some(q) = query {
            let pattern = format!("%{}%", q);
            let mut rows = self
                .conn
                .query(
                    "SELECT id, kind, name, username, last_message_ts, is_forum FROM chats
                     WHERE name LIKE ?1 OR username LIKE ?1
                     ORDER BY last_message_ts DESC LIMIT ?2",
                    (pattern.as_str(), limit),
                )
                .await?;
            while let Some(row) = rows.next().await? {
                chats.push(row_to_chat(&row)?);
            }
        } else {
            let mut rows = self
                .conn
                .query(
                    "SELECT id, kind, name, username, last_message_ts, is_forum FROM chats
                     ORDER BY last_message_ts DESC LIMIT ?1",
                    [limit],
                )
                .await?;
            while let Some(row) = rows.next().await? {
                chats.push(row_to_chat(&row)?);
            }
        }
        Ok(chats)
    }

    pub async fn get_chat(&self, id: i64) -> Result<Option<Chat>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, kind, name, username, last_message_ts, is_forum FROM chats WHERE id = ?1",
                [id],
            )
            .await?;
        if let Some(row) = rows.next().await? {
            Ok(Some(row_to_chat(&row)?))
        } else {
            Ok(None)
        }
    }

    /// Delete a chat from local database. Returns true if a chat was deleted.
    pub async fn delete_chat(&self, id: i64) -> Result<bool> {
        let affected = self
            .conn
            .execute("DELETE FROM chats WHERE id = ?1", [id])
            .await?;
        Ok(affected > 0)
    }

    /// Delete all messages for a chat from local database. Returns count of deleted messages.
    pub async fn delete_messages_by_chat(&self, chat_id: i64) -> Result<u64> {
        let affected = self
            .conn
            .execute("DELETE FROM messages WHERE chat_id = ?1", [chat_id])
            .await?;
        Ok(affected)
    }

    // --- Topics ---

    pub async fn upsert_topic(
        &self,
        chat_id: i64,
        topic_id: i32,
        name: &str,
        icon_color: i32,
        icon_emoji: Option<&str>,
    ) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO topics (chat_id, topic_id, name, icon_color, icon_emoji)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(chat_id, topic_id) DO UPDATE SET
                    name = CASE WHEN excluded.name != '' THEN excluded.name ELSE name END,
                    icon_color = excluded.icon_color,
                    icon_emoji = COALESCE(excluded.icon_emoji, icon_emoji)",
                (chat_id, topic_id, name, icon_color, icon_emoji),
            )
            .await?;
        Ok(())
    }

    pub async fn list_topics(&self, chat_id: i64) -> Result<Vec<Topic>> {
        let mut rows = self
            .conn
            .query(
                "SELECT chat_id, topic_id, name, icon_color, icon_emoji FROM topics
                 WHERE chat_id = ?1 ORDER BY topic_id",
                [chat_id],
            )
            .await?;
        let mut topics = Vec::new();
        while let Some(row) = rows.next().await? {
            topics.push(row_to_topic(&row)?);
        }
        Ok(topics)
    }

    pub async fn get_topic(&self, chat_id: i64, topic_id: i32) -> Result<Option<Topic>> {
        let mut rows = self
            .conn
            .query(
                "SELECT chat_id, topic_id, name, icon_color, icon_emoji FROM topics
                 WHERE chat_id = ?1 AND topic_id = ?2",
                (chat_id, topic_id),
            )
            .await?;
        if let Some(row) = rows.next().await? {
            Ok(Some(row_to_topic(&row)?))
        } else {
            Ok(None)
        }
    }

    // --- Contacts ---

    pub async fn upsert_contact(
        &self,
        user_id: i64,
        username: Option<&str>,
        first_name: &str,
        last_name: &str,
        phone: &str,
    ) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO contacts (user_id, username, first_name, last_name, phone)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(user_id) DO UPDATE SET
                    username = COALESCE(excluded.username, username),
                    first_name = CASE WHEN excluded.first_name != '' THEN excluded.first_name ELSE first_name END,
                    last_name = CASE WHEN excluded.last_name != '' THEN excluded.last_name ELSE last_name END,
                    phone = CASE WHEN excluded.phone != '' THEN excluded.phone ELSE phone END",
                (user_id, username, first_name, last_name, phone),
            )
            .await?;
        Ok(())
    }

    pub async fn search_contacts(&self, query: &str, limit: i64) -> Result<Vec<Contact>> {
        let pattern = format!("%{}%", query);
        let mut rows = self
            .conn
            .query(
                "SELECT user_id, username, first_name, last_name, phone FROM contacts
                 WHERE first_name LIKE ?1 OR last_name LIKE ?1 OR username LIKE ?1 OR phone LIKE ?1
                 ORDER BY first_name LIMIT ?2",
                (pattern.as_str(), limit),
            )
            .await?;
        let mut contacts = Vec::new();
        while let Some(row) = rows.next().await? {
            contacts.push(row_to_contact(&row)?);
        }
        Ok(contacts)
    }

    pub async fn get_contact(&self, user_id: i64) -> Result<Option<Contact>> {
        let mut rows = self
            .conn
            .query(
                "SELECT user_id, username, first_name, last_name, phone FROM contacts WHERE user_id = ?1",
                [user_id],
            )
            .await?;
        if let Some(row) = rows.next().await? {
            Ok(Some(row_to_contact(&row)?))
        } else {
            Ok(None)
        }
    }

    // --- Messages ---

    pub async fn upsert_message(&self, p: UpsertMessageParams) -> Result<()> {
        let ts_str = p.ts.to_rfc3339();
        let edit_ts_str = p.edit_ts.map(|t| t.to_rfc3339());
        let from_me_int = p.from_me as i64;

        self.conn
            .execute(
                "INSERT INTO messages (id, chat_id, sender_id, ts, edit_ts, from_me, text, media_type, media_path, reply_to_id, topic_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(chat_id, id) DO UPDATE SET
                    sender_id = excluded.sender_id,
                    ts = excluded.ts,
                    edit_ts = COALESCE(excluded.edit_ts, edit_ts),
                    from_me = excluded.from_me,
                    text = CASE WHEN excluded.text != '' THEN excluded.text ELSE text END,
                    media_type = COALESCE(excluded.media_type, media_type),
                    media_path = COALESCE(excluded.media_path, media_path),
                    reply_to_id = COALESCE(excluded.reply_to_id, reply_to_id),
                    topic_id = COALESCE(excluded.topic_id, topic_id)",
                (
                    p.id,
                    p.chat_id,
                    p.sender_id,
                    ts_str.as_str(),
                    edit_ts_str.as_deref(),
                    from_me_int,
                    p.text.as_str(),
                    p.media_type.as_deref(),
                    p.media_path.as_deref(),
                    p.reply_to_id,
                    p.topic_id,
                ),
            )
            .await?;
        Ok(())
    }

    pub async fn list_messages(&self, p: ListMessagesParams) -> Result<Vec<Message>> {
        // Build dynamic SQL using positional parameters
        let mut conditions = vec!["1=1".to_string()];
        let mut param_idx = 1;

        // We'll build the SQL with positional params and collect values
        // Due to turso's typed params, we'll use a simpler approach with string formatting
        // for the dynamic WHERE clause, but still use params for the actual values

        let chat_filter = p.chat_id.map(|_| {
            let cond = format!("m.chat_id = ?{}", param_idx);
            param_idx += 1;
            cond
        });
        if let Some(c) = &chat_filter {
            conditions.push(c.clone());
        }

        let topic_filter = p.topic_id.map(|_| {
            let cond = format!("m.topic_id = ?{}", param_idx);
            param_idx += 1;
            cond
        });
        if let Some(c) = &topic_filter {
            conditions.push(c.clone());
        }

        let after_filter = p.after.as_ref().map(|_| {
            let cond = format!("m.ts > ?{}", param_idx);
            param_idx += 1;
            cond
        });
        if let Some(c) = &after_filter {
            conditions.push(c.clone());
        }

        let before_filter = p.before.as_ref().map(|_| {
            let cond = format!("m.ts < ?{}", param_idx);
            param_idx += 1;
            cond
        });
        if let Some(c) = &before_filter {
            conditions.push(c.clone());
        }

        // For ignore_chats, we'll use NOT IN with literal values (safe since they're i64)
        if !p.ignore_chats.is_empty() {
            let ids: Vec<String> = p.ignore_chats.iter().map(|id| id.to_string()).collect();
            conditions.push(format!("m.chat_id NOT IN ({})", ids.join(",")));
        }

        if p.ignore_channels {
            conditions.push("COALESCE(c.kind, '') != 'channel'".to_string());
        }

        // Limit param
        let limit_param_idx = param_idx;

        let sql = format!(
            "SELECT m.id, m.chat_id, m.sender_id, m.ts, m.edit_ts, m.from_me, m.text, m.media_type, m.media_path, m.reply_to_id, m.topic_id
             FROM messages m
             LEFT JOIN chats c ON c.id = m.chat_id
             WHERE {} ORDER BY m.ts DESC LIMIT ?{}",
            conditions.join(" AND "),
            limit_param_idx
        );

        // Build params tuple dynamically - we need to handle this carefully
        // Using a Vec<turso::Value> approach
        use turso::Value;
        let mut params: Vec<Value> = Vec::new();

        if let Some(chat_id) = p.chat_id {
            params.push(Value::Integer(chat_id));
        }
        if let Some(topic_id) = p.topic_id {
            params.push(Value::Integer(topic_id as i64));
        }
        if let Some(ref after) = p.after {
            params.push(Value::Text(after.to_rfc3339()));
        }
        if let Some(ref before) = p.before {
            params.push(Value::Text(before.to_rfc3339()));
        }
        params.push(Value::Integer(p.limit));

        let mut rows = self
            .conn
            .query(&sql, turso::params_from_iter(params))
            .await?;

        let mut msgs = Vec::new();
        while let Some(row) = rows.next().await? {
            msgs.push(row_to_message(&row)?);
        }
        msgs.reverse(); // chronological order
        Ok(msgs)
    }

    pub async fn search_messages(&self, p: SearchMessagesParams) -> Result<Vec<Message>> {
        if self.has_fts {
            self.search_messages_fts(p).await
        } else {
            self.search_messages_like(p).await
        }
    }

    async fn search_messages_fts(&self, p: SearchMessagesParams) -> Result<Vec<Message>> {
        use turso::Value;

        let mut conditions = vec!["messages_fts MATCH ?1".to_string()];
        let mut params: Vec<Value> = vec![Value::Text(p.query.clone())];
        let mut param_idx = 2;

        if let Some(chat_id) = p.chat_id {
            conditions.push(format!("m.chat_id = ?{}", param_idx));
            params.push(Value::Integer(chat_id));
            param_idx += 1;
        }
        if let Some(from_id) = p.from_id {
            conditions.push(format!("m.sender_id = ?{}", param_idx));
            params.push(Value::Integer(from_id));
            param_idx += 1;
        }
        if let Some(ref media_type) = p.media_type {
            conditions.push(format!("m.media_type = ?{}", param_idx));
            params.push(Value::Text(media_type.clone()));
            param_idx += 1;
        }

        if !p.ignore_chats.is_empty() {
            let ids: Vec<String> = p.ignore_chats.iter().map(|id| id.to_string()).collect();
            conditions.push(format!("m.chat_id NOT IN ({})", ids.join(",")));
        }
        if p.ignore_channels {
            conditions.push("COALESCE(c.kind, '') != 'channel'".to_string());
        }

        let sql = format!(
            "SELECT m.id, m.chat_id, m.sender_id, m.ts, m.edit_ts, m.from_me, m.text, m.media_type, m.media_path, m.reply_to_id, m.topic_id,
                    snippet(messages_fts, 0, '»', '«', '…', 40) as snippet
             FROM messages m
             JOIN messages_fts ON messages_fts.rowid = m.rowid
             LEFT JOIN chats c ON c.id = m.chat_id
             WHERE {} ORDER BY m.ts DESC LIMIT ?{}",
            conditions.join(" AND "),
            param_idx
        );
        params.push(Value::Integer(p.limit));

        let mut rows = self
            .conn
            .query(&sql, turso::params_from_iter(params))
            .await?;

        let mut msgs = Vec::new();
        while let Some(row) = rows.next().await? {
            let mut m = row_to_message(&row)?;
            m.snippet = row.get::<String>(11).unwrap_or_default();
            msgs.push(m);
        }
        Ok(msgs)
    }

    async fn search_messages_like(&self, p: SearchMessagesParams) -> Result<Vec<Message>> {
        use turso::Value;

        let pattern = format!("%{}%", p.query);
        let mut conditions = vec!["m.text LIKE ?1".to_string()];
        let mut params: Vec<Value> = vec![Value::Text(pattern)];
        let mut param_idx = 2;

        if let Some(chat_id) = p.chat_id {
            conditions.push(format!("m.chat_id = ?{}", param_idx));
            params.push(Value::Integer(chat_id));
            param_idx += 1;
        }
        if let Some(from_id) = p.from_id {
            conditions.push(format!("m.sender_id = ?{}", param_idx));
            params.push(Value::Integer(from_id));
            param_idx += 1;
        }
        if let Some(ref media_type) = p.media_type {
            conditions.push(format!("m.media_type = ?{}", param_idx));
            params.push(Value::Text(media_type.clone()));
            param_idx += 1;
        }

        if !p.ignore_chats.is_empty() {
            let ids: Vec<String> = p.ignore_chats.iter().map(|id| id.to_string()).collect();
            conditions.push(format!("m.chat_id NOT IN ({})", ids.join(",")));
        }
        if p.ignore_channels {
            conditions.push("COALESCE(c.kind, '') != 'channel'".to_string());
        }

        let sql = format!(
            "SELECT m.id, m.chat_id, m.sender_id, m.ts, m.edit_ts, m.from_me, m.text, m.media_type, m.media_path, m.reply_to_id, m.topic_id
             FROM messages m
             LEFT JOIN chats c ON c.id = m.chat_id
             WHERE {} ORDER BY m.ts DESC LIMIT ?{}",
            conditions.join(" AND "),
            param_idx
        );
        params.push(Value::Integer(p.limit));

        let mut rows = self
            .conn
            .query(&sql, turso::params_from_iter(params))
            .await?;

        let mut msgs = Vec::new();
        while let Some(row) = rows.next().await? {
            msgs.push(row_to_message(&row)?);
        }
        Ok(msgs)
    }

    pub async fn message_context(
        &self,
        chat_id: i64,
        msg_id: i64,
        before: i64,
        after: i64,
    ) -> Result<Vec<Message>> {
        // Get the target message timestamp
        let mut ts_rows = self
            .conn
            .query(
                "SELECT ts FROM messages WHERE chat_id = ?1 AND id = ?2",
                (chat_id, msg_id),
            )
            .await?;
        let ts: String = match ts_rows.next().await? {
            Some(row) => row.get(0)?,
            None => anyhow::bail!("Message {}/{} not found", chat_id, msg_id),
        };

        // Messages before
        let mut before_rows = self
            .conn
            .query(
                "SELECT id, chat_id, sender_id, ts, edit_ts, from_me, text, media_type, media_path, reply_to_id, topic_id
                 FROM messages WHERE chat_id = ?1 AND ts < ?2 ORDER BY ts DESC LIMIT ?3",
                (chat_id, ts.as_str(), before),
            )
            .await?;
        let mut before_msgs = Vec::new();
        while let Some(row) = before_rows.next().await? {
            before_msgs.push(row_to_message(&row)?);
        }
        before_msgs.reverse();

        // The target message
        let mut target_rows = self
            .conn
            .query(
                "SELECT id, chat_id, sender_id, ts, edit_ts, from_me, text, media_type, media_path, reply_to_id, topic_id
                 FROM messages WHERE chat_id = ?1 AND id = ?2",
                (chat_id, msg_id),
            )
            .await?;
        let target = match target_rows.next().await? {
            Some(row) => row_to_message(&row)?,
            None => anyhow::bail!("Message not found"),
        };

        // Messages after
        let mut after_rows = self
            .conn
            .query(
                "SELECT id, chat_id, sender_id, ts, edit_ts, from_me, text, media_type, media_path, reply_to_id, topic_id
                 FROM messages WHERE chat_id = ?1 AND ts > ?2 ORDER BY ts ASC LIMIT ?3",
                (chat_id, ts.as_str(), after),
            )
            .await?;
        let mut after_msgs = Vec::new();
        while let Some(row) = after_rows.next().await? {
            after_msgs.push(row_to_message(&row)?);
        }

        let mut result = before_msgs;
        result.push(target);
        result.extend(after_msgs);
        Ok(result)
    }

    /// Update a message's text (for edits)
    pub async fn update_message_text(&self, chat_id: i64, msg_id: i64, new_text: &str) -> Result<()> {
        let edit_ts = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "UPDATE messages SET text = ?1, edit_ts = ?2 WHERE chat_id = ?3 AND id = ?4",
                (new_text, edit_ts.as_str(), chat_id, msg_id),
            )
            .await?;
        Ok(())
    }

    pub async fn get_message(&self, chat_id: i64, msg_id: i64) -> Result<Option<Message>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, chat_id, sender_id, ts, edit_ts, from_me, text, media_type, media_path, reply_to_id, topic_id
                 FROM messages WHERE chat_id = ?1 AND id = ?2",
                (chat_id, msg_id),
            )
            .await?;
        if let Some(row) = rows.next().await? {
            Ok(Some(row_to_message(&row)?))
        } else {
            Ok(None)
        }
    }

    /// Get the oldest message ID for a chat (lowest message ID).
    /// Returns None if no messages exist for the chat.
    pub async fn get_oldest_message_id(
        &self,
        chat_id: i64,
        topic_id: Option<i32>,
    ) -> Result<Option<i64>> {
        let mut rows = if let Some(tid) = topic_id {
            self.conn
                .query(
                    "SELECT MIN(id) FROM messages WHERE chat_id = ?1 AND topic_id = ?2",
                    (chat_id, tid),
                )
                .await?
        } else {
            self.conn
                .query("SELECT MIN(id) FROM messages WHERE chat_id = ?1", [chat_id])
                .await?
        };
        if let Some(row) = rows.next().await? {
            Ok(row.get::<Option<i64>>(0)?)
        } else {
            Ok(None)
        }
    }
}

fn parse_ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn row_to_chat(row: &Row) -> Result<Chat> {
    Ok(Chat {
        id: row.get(0)?,
        kind: row.get(1)?,
        name: row.get(2)?,
        username: row.get::<Option<String>>(3)?,
        last_message_ts: row.get::<Option<String>>(4)?.map(|s| parse_ts(&s)),
        is_forum: row.get::<i64>(5).unwrap_or(0) != 0,
    })
}

fn row_to_contact(row: &Row) -> Result<Contact> {
    Ok(Contact {
        user_id: row.get(0)?,
        username: row.get::<Option<String>>(1)?,
        first_name: row.get(2)?,
        last_name: row.get(3)?,
        phone: row.get(4)?,
    })
}

fn row_to_topic(row: &Row) -> Result<Topic> {
    Ok(Topic {
        chat_id: row.get(0)?,
        topic_id: row.get(1)?,
        name: row.get(2)?,
        icon_color: row.get(3)?,
        icon_emoji: row.get::<Option<String>>(4)?,
    })
}

fn row_to_message(row: &Row) -> Result<Message> {
    Ok(Message {
        id: row.get(0)?,
        chat_id: row.get(1)?,
        sender_id: row.get(2)?,
        ts: row.get::<String>(3).map(|s| parse_ts(&s))?,
        edit_ts: row.get::<Option<String>>(4)?.map(|s| parse_ts(&s)),
        from_me: row.get::<i64>(5)? != 0,
        text: row.get(6)?,
        media_type: row.get::<Option<String>>(7)?,
        media_path: row.get::<Option<String>>(8)?,
        reply_to_id: row.get::<Option<i64>>(9)?,
        topic_id: row.get::<Option<i32>>(10).ok().flatten(),
        snippet: String::new(),
    })
}
