use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlite::{Connection, State, Value};
use std::path::Path;

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
    pub reply_to_id: Option<i64>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub snippet: String,
}

pub struct ListMessagesParams {
    pub chat_id: Option<i64>,
    pub limit: i64,
    pub after: Option<DateTime<Utc>>,
    pub before: Option<DateTime<Utc>>,
}

pub struct SearchMessagesParams {
    pub query: String,
    pub chat_id: Option<i64>,
    pub from_id: Option<i64>,
    pub limit: i64,
    pub media_type: Option<String>,
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
    pub reply_to_id: Option<i64>,
}

impl Store {
    pub fn open(store_dir: &str) -> Result<Self> {
        std::fs::create_dir_all(store_dir)?;
        let db_path = Path::new(store_dir).join("tgrs.db");
        let conn = Connection::open(&db_path)?;

        conn.execute("PRAGMA journal_mode=WAL")?;
        conn.execute("PRAGMA busy_timeout=5000")?;

        let mut store = Store {
            conn,
            has_fts: false,
        };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&mut self) -> Result<()> {
        self.conn
            .execute(
                "
            CREATE TABLE IF NOT EXISTS chats (
                id INTEGER PRIMARY KEY,
                kind TEXT NOT NULL DEFAULT 'user',
                name TEXT NOT NULL DEFAULT '',
                username TEXT,
                last_message_ts TEXT
            );

            CREATE TABLE IF NOT EXISTS contacts (
                user_id INTEGER PRIMARY KEY,
                username TEXT,
                first_name TEXT NOT NULL DEFAULT '',
                last_name TEXT NOT NULL DEFAULT '',
                phone TEXT NOT NULL DEFAULT ''
            );

            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER NOT NULL,
                chat_id INTEGER NOT NULL,
                sender_id INTEGER NOT NULL DEFAULT 0,
                ts TEXT NOT NULL,
                edit_ts TEXT,
                from_me INTEGER NOT NULL DEFAULT 0,
                text TEXT NOT NULL DEFAULT '',
                media_type TEXT,
                reply_to_id INTEGER,
                PRIMARY KEY (chat_id, id)
            );

            CREATE INDEX IF NOT EXISTS idx_messages_chat_ts ON messages(chat_id, ts);
            CREATE INDEX IF NOT EXISTS idx_messages_ts ON messages(ts);
            CREATE INDEX IF NOT EXISTS idx_messages_sender ON messages(sender_id);
            ",
            )
            .context("Failed to create base tables")?;

        // Try to create FTS5 table
        let fts_result = self.conn.execute(
            "
            CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
                text,
                content='messages',
                content_rowid='rowid'
            );

            CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
                INSERT INTO messages_fts(rowid, text) VALUES (new.rowid, new.text);
            END;
            CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
                INSERT INTO messages_fts(messages_fts, rowid, text) VALUES('delete', old.rowid, old.text);
            END;
            CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE ON messages BEGIN
                INSERT INTO messages_fts(messages_fts, rowid, text) VALUES('delete', old.rowid, old.text);
                INSERT INTO messages_fts(rowid, text) VALUES (new.rowid, new.text);
            END;
            ",
        );

        self.has_fts = fts_result.is_ok();
        if !self.has_fts {
            log::warn!("FTS5 not available, search will use LIKE fallback");
        }

        Ok(())
    }

    pub fn has_fts(&self) -> bool {
        self.has_fts
    }

    // --- Chats ---

    pub fn upsert_chat(
        &self,
        id: i64,
        kind: &str,
        name: &str,
        username: Option<&str>,
        last_message_ts: Option<DateTime<Utc>>,
    ) -> Result<()> {
        let ts_str = last_message_ts.map(|t| t.to_rfc3339());
        let mut stmt = self.conn.prepare(
            "INSERT INTO chats (id, kind, name, username, last_message_ts)
             VALUES (:id, :kind, :name, :username, :ts)
             ON CONFLICT(id) DO UPDATE SET
                kind = COALESCE(:kind, kind),
                name = CASE WHEN :name != '' THEN :name ELSE name END,
                username = COALESCE(:username, username),
                last_message_ts = CASE WHEN :ts IS NOT NULL AND (:ts > last_message_ts OR last_message_ts IS NULL)
                    THEN :ts ELSE last_message_ts END",
        )?;
        stmt.bind::<&[(_, Value)]>(
            &[
                (":id", id.into()),
                (":kind", kind.into()),
                (":name", name.into()),
                (
                    ":username",
                    username.map(Value::from).unwrap_or(Value::Null),
                ),
                (":ts", ts_str.map(Value::from).unwrap_or(Value::Null)),
            ][..],
        )?;
        stmt.next()?;
        Ok(())
    }

    pub fn list_chats(&self, query: Option<&str>, limit: i64) -> Result<Vec<Chat>> {
        let mut chats = Vec::new();

        if let Some(q) = query {
            let pattern = format!("%{}%", q);
            let mut stmt = self.conn.prepare(
                "SELECT id, kind, name, username, last_message_ts FROM chats
                 WHERE name LIKE :pat OR username LIKE :pat
                 ORDER BY last_message_ts DESC LIMIT :limit",
            )?;
            stmt.bind::<&[(_, Value)]>(
                &[(":pat", pattern.into()), (":limit", limit.into())][..],
            )?;
            while let Ok(State::Row) = stmt.next() {
                chats.push(row_to_chat(&stmt));
            }
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT id, kind, name, username, last_message_ts FROM chats
                 ORDER BY last_message_ts DESC LIMIT :limit",
            )?;
            stmt.bind::<&[(_, Value)]>(&[(":limit", limit.into())][..])?;
            while let Ok(State::Row) = stmt.next() {
                chats.push(row_to_chat(&stmt));
            }
        }
        Ok(chats)
    }

    pub fn get_chat(&self, id: i64) -> Result<Option<Chat>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, name, username, last_message_ts FROM chats WHERE id = :id",
        )?;
        stmt.bind::<&[(_, Value)]>(&[(":id", id.into())][..])?;
        if let Ok(State::Row) = stmt.next() {
            Ok(Some(row_to_chat(&stmt)))
        } else {
            Ok(None)
        }
    }

    // --- Contacts ---

    pub fn upsert_contact(
        &self,
        user_id: i64,
        username: Option<&str>,
        first_name: &str,
        last_name: &str,
        phone: &str,
    ) -> Result<()> {
        let mut stmt = self.conn.prepare(
            "INSERT INTO contacts (user_id, username, first_name, last_name, phone)
             VALUES (:uid, :uname, :fname, :lname, :phone)
             ON CONFLICT(user_id) DO UPDATE SET
                username = COALESCE(:uname, username),
                first_name = CASE WHEN :fname != '' THEN :fname ELSE first_name END,
                last_name = CASE WHEN :lname != '' THEN :lname ELSE last_name END,
                phone = CASE WHEN :phone != '' THEN :phone ELSE phone END",
        )?;
        stmt.bind::<&[(_, Value)]>(
            &[
                (":uid", user_id.into()),
                (
                    ":uname",
                    username.map(Value::from).unwrap_or(Value::Null),
                ),
                (":fname", first_name.into()),
                (":lname", last_name.into()),
                (":phone", phone.into()),
            ][..],
        )?;
        stmt.next()?;
        Ok(())
    }

    pub fn search_contacts(&self, query: &str, limit: i64) -> Result<Vec<Contact>> {
        let pattern = format!("%{}%", query);
        let mut stmt = self.conn.prepare(
            "SELECT user_id, username, first_name, last_name, phone FROM contacts
             WHERE first_name LIKE :pat OR last_name LIKE :pat OR username LIKE :pat OR phone LIKE :pat
             ORDER BY first_name LIMIT :limit",
        )?;
        stmt.bind::<&[(_, Value)]>(
            &[(":pat", pattern.into()), (":limit", limit.into())][..],
        )?;
        let mut contacts = Vec::new();
        while let Ok(State::Row) = stmt.next() {
            contacts.push(row_to_contact(&stmt));
        }
        Ok(contacts)
    }

    pub fn get_contact(&self, user_id: i64) -> Result<Option<Contact>> {
        let mut stmt = self.conn.prepare(
            "SELECT user_id, username, first_name, last_name, phone FROM contacts WHERE user_id = :uid",
        )?;
        stmt.bind::<&[(_, Value)]>(&[(":uid", user_id.into())][..])?;
        if let Ok(State::Row) = stmt.next() {
            Ok(Some(row_to_contact(&stmt)))
        } else {
            Ok(None)
        }
    }

    // --- Messages ---

    pub fn upsert_message(&self, p: UpsertMessageParams) -> Result<()> {
        let ts_str: Value = p.ts.to_rfc3339().into();
        let edit_ts_str: Value = p
            .edit_ts
            .map(|t| Value::from(t.to_rfc3339()))
            .unwrap_or(Value::Null);
        let media: Value = p
            .media_type
            .as_deref()
            .map(Value::from)
            .unwrap_or(Value::Null);
        let reply_to: Value = p.reply_to_id.map(Value::from).unwrap_or(Value::Null);

        let mut stmt = self.conn.prepare(
            "INSERT INTO messages (id, chat_id, sender_id, ts, edit_ts, from_me, text, media_type, reply_to_id)
             VALUES (:id, :chat_id, :sender_id, :ts, :edit_ts, :from_me, :text, :media_type, :reply_to_id)
             ON CONFLICT(chat_id, id) DO UPDATE SET
                sender_id = :sender_id,
                ts = :ts,
                edit_ts = COALESCE(:edit_ts, edit_ts),
                from_me = :from_me,
                text = CASE WHEN :text != '' THEN :text ELSE text END,
                media_type = COALESCE(:media_type, media_type),
                reply_to_id = COALESCE(:reply_to_id, reply_to_id)",
        )?;
        stmt.bind::<&[(_, Value)]>(
            &[
                (":id", p.id.into()),
                (":chat_id", p.chat_id.into()),
                (":sender_id", p.sender_id.into()),
                (":ts", ts_str),
                (":edit_ts", edit_ts_str),
                (":from_me", (p.from_me as i64).into()),
                (":text", p.text.as_str().into()),
                (":media_type", media),
                (":reply_to_id", reply_to),
            ][..],
        )?;
        stmt.next()?;
        Ok(())
    }

    pub fn list_messages(&self, p: ListMessagesParams) -> Result<Vec<Message>> {
        // Build dynamic SQL
        let mut conditions = vec!["1=1".to_string()];
        let mut binds: Vec<(String, Value)> = Vec::new();

        if let Some(chat_id) = p.chat_id {
            conditions.push("chat_id = :chat_id".to_string());
            binds.push((":chat_id".to_string(), chat_id.into()));
        }
        if let Some(ref after) = p.after {
            conditions.push("ts > :after".to_string());
            binds.push((":after".to_string(), after.to_rfc3339().into()));
        }
        if let Some(ref before) = p.before {
            conditions.push("ts < :before".to_string());
            binds.push((":before".to_string(), before.to_rfc3339().into()));
        }

        let sql = format!(
            "SELECT id, chat_id, sender_id, ts, edit_ts, from_me, text, media_type, reply_to_id
             FROM messages WHERE {} ORDER BY ts DESC LIMIT :limit",
            conditions.join(" AND ")
        );
        binds.push((":limit".to_string(), p.limit.into()));

        let mut stmt = self.conn.prepare(&sql)?;
        let bind_refs: Vec<(&str, Value)> = binds.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
        stmt.bind::<&[(&str, Value)]>(&bind_refs)?;

        let mut msgs = Vec::new();
        while let Ok(State::Row) = stmt.next() {
            msgs.push(row_to_message(&stmt));
        }
        msgs.reverse(); // chronological order
        Ok(msgs)
    }

    pub fn search_messages(&self, p: SearchMessagesParams) -> Result<Vec<Message>> {
        if self.has_fts {
            self.search_messages_fts(p)
        } else {
            self.search_messages_like(p)
        }
    }

    fn search_messages_fts(&self, p: SearchMessagesParams) -> Result<Vec<Message>> {
        let mut conditions = vec!["messages_fts MATCH :query".to_string()];
        let mut binds: Vec<(String, Value)> = vec![(":query".to_string(), p.query.clone().into())];

        if let Some(chat_id) = p.chat_id {
            conditions.push("m.chat_id = :chat_id".to_string());
            binds.push((":chat_id".to_string(), chat_id.into()));
        }
        if let Some(from_id) = p.from_id {
            conditions.push("m.sender_id = :from_id".to_string());
            binds.push((":from_id".to_string(), from_id.into()));
        }
        if let Some(ref media_type) = p.media_type {
            conditions.push("m.media_type = :media_type".to_string());
            binds.push((":media_type".to_string(), media_type.clone().into()));
        }

        let sql = format!(
            "SELECT m.id, m.chat_id, m.sender_id, m.ts, m.edit_ts, m.from_me, m.text, m.media_type, m.reply_to_id,
                    snippet(messages_fts, 0, '»', '«', '…', 40) as snippet
             FROM messages m
             JOIN messages_fts ON messages_fts.rowid = m.rowid
             WHERE {} ORDER BY m.ts DESC LIMIT :limit",
            conditions.join(" AND ")
        );
        binds.push((":limit".to_string(), p.limit.into()));

        let mut stmt = self.conn.prepare(&sql)?;
        let bind_refs: Vec<(&str, Value)> = binds.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
        stmt.bind::<&[(&str, Value)]>(&bind_refs)?;

        let mut msgs = Vec::new();
        while let Ok(State::Row) = stmt.next() {
            let mut m = row_to_message(&stmt);
            m.snippet = stmt.read::<String, _>("snippet").unwrap_or_default();
            msgs.push(m);
        }
        Ok(msgs)
    }

    fn search_messages_like(&self, p: SearchMessagesParams) -> Result<Vec<Message>> {
        let pattern = format!("%{}%", p.query);
        let mut conditions = vec!["text LIKE :pat".to_string()];
        let mut binds: Vec<(String, Value)> = vec![(":pat".to_string(), pattern.into())];

        if let Some(chat_id) = p.chat_id {
            conditions.push("chat_id = :chat_id".to_string());
            binds.push((":chat_id".to_string(), chat_id.into()));
        }
        if let Some(from_id) = p.from_id {
            conditions.push("sender_id = :from_id".to_string());
            binds.push((":from_id".to_string(), from_id.into()));
        }
        if let Some(ref media_type) = p.media_type {
            conditions.push("media_type = :media_type".to_string());
            binds.push((":media_type".to_string(), media_type.clone().into()));
        }

        let sql = format!(
            "SELECT id, chat_id, sender_id, ts, edit_ts, from_me, text, media_type, reply_to_id
             FROM messages WHERE {} ORDER BY ts DESC LIMIT :limit",
            conditions.join(" AND ")
        );
        binds.push((":limit".to_string(), p.limit.into()));

        let mut stmt = self.conn.prepare(&sql)?;
        let bind_refs: Vec<(&str, Value)> = binds.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
        stmt.bind::<&[(&str, Value)]>(&bind_refs)?;

        let mut msgs = Vec::new();
        while let Ok(State::Row) = stmt.next() {
            msgs.push(row_to_message(&stmt));
        }
        Ok(msgs)
    }

    pub fn message_context(
        &self,
        chat_id: i64,
        msg_id: i64,
        before: i64,
        after: i64,
    ) -> Result<Vec<Message>> {
        // Get the target message timestamp
        let mut ts_stmt = self
            .conn
            .prepare("SELECT ts FROM messages WHERE chat_id = :chat AND id = :id")?;
        ts_stmt.bind::<&[(_, Value)]>(
            &[(":chat", chat_id.into()), (":id", msg_id.into())][..],
        )?;
        if ts_stmt.next()? != State::Row {
            anyhow::bail!("Message {}/{} not found", chat_id, msg_id);
        }
        let ts: String = ts_stmt.read("ts")?;

        // Messages before
        let mut before_stmt = self.conn.prepare(
            "SELECT id, chat_id, sender_id, ts, edit_ts, from_me, text, media_type, reply_to_id
             FROM messages WHERE chat_id = :chat AND ts < :ts ORDER BY ts DESC LIMIT :limit",
        )?;
        before_stmt.bind::<&[(_, Value)]>(
            &[
                (":chat", chat_id.into()),
                (":ts", ts.clone().into()),
                (":limit", before.into()),
            ][..],
        )?;
        let mut before_msgs = Vec::new();
        while let Ok(State::Row) = before_stmt.next() {
            before_msgs.push(row_to_message(&before_stmt));
        }
        before_msgs.reverse();

        // The target message
        let mut target_stmt = self.conn.prepare(
            "SELECT id, chat_id, sender_id, ts, edit_ts, from_me, text, media_type, reply_to_id
             FROM messages WHERE chat_id = :chat AND id = :id",
        )?;
        target_stmt.bind::<&[(_, Value)]>(
            &[(":chat", chat_id.into()), (":id", msg_id.into())][..],
        )?;
        let target = if let Ok(State::Row) = target_stmt.next() {
            row_to_message(&target_stmt)
        } else {
            anyhow::bail!("Message not found");
        };

        // Messages after
        let mut after_stmt = self.conn.prepare(
            "SELECT id, chat_id, sender_id, ts, edit_ts, from_me, text, media_type, reply_to_id
             FROM messages WHERE chat_id = :chat AND ts > :ts ORDER BY ts ASC LIMIT :limit",
        )?;
        after_stmt.bind::<&[(_, Value)]>(
            &[
                (":chat", chat_id.into()),
                (":ts", ts.into()),
                (":limit", after.into()),
            ][..],
        )?;
        let mut after_msgs = Vec::new();
        while let Ok(State::Row) = after_stmt.next() {
            after_msgs.push(row_to_message(&after_stmt));
        }

        let mut result = before_msgs;
        result.push(target);
        result.extend(after_msgs);
        Ok(result)
    }

    pub fn get_message(&self, chat_id: i64, msg_id: i64) -> Result<Option<Message>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, chat_id, sender_id, ts, edit_ts, from_me, text, media_type, reply_to_id
             FROM messages WHERE chat_id = :chat AND id = :id",
        )?;
        stmt.bind::<&[(_, Value)]>(
            &[(":chat", chat_id.into()), (":id", msg_id.into())][..],
        )?;
        if let Ok(State::Row) = stmt.next() {
            Ok(Some(row_to_message(&stmt)))
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

fn row_to_chat(stmt: &sqlite::Statement) -> Chat {
    Chat {
        id: stmt.read::<i64, _>("id").unwrap_or(0),
        kind: stmt.read::<String, _>("kind").unwrap_or_default(),
        name: stmt.read::<String, _>("name").unwrap_or_default(),
        username: stmt.read::<Option<String>, _>("username").unwrap_or(None),
        last_message_ts: stmt
            .read::<Option<String>, _>("last_message_ts")
            .unwrap_or(None)
            .map(|s| parse_ts(&s)),
    }
}

fn row_to_contact(stmt: &sqlite::Statement) -> Contact {
    Contact {
        user_id: stmt.read::<i64, _>("user_id").unwrap_or(0),
        username: stmt.read::<Option<String>, _>("username").unwrap_or(None),
        first_name: stmt.read::<String, _>("first_name").unwrap_or_default(),
        last_name: stmt.read::<String, _>("last_name").unwrap_or_default(),
        phone: stmt.read::<String, _>("phone").unwrap_or_default(),
    }
}

fn row_to_message(stmt: &sqlite::Statement) -> Message {
    Message {
        id: stmt.read::<i64, _>("id").unwrap_or(0),
        chat_id: stmt.read::<i64, _>("chat_id").unwrap_or(0),
        sender_id: stmt.read::<i64, _>("sender_id").unwrap_or(0),
        ts: stmt
            .read::<String, _>("ts")
            .map(|s| parse_ts(&s))
            .unwrap_or_else(|_| Utc::now()),
        edit_ts: stmt
            .read::<Option<String>, _>("edit_ts")
            .unwrap_or(None)
            .map(|s| parse_ts(&s)),
        from_me: stmt.read::<i64, _>("from_me").unwrap_or(0) != 0,
        text: stmt.read::<String, _>("text").unwrap_or_default(),
        media_type: stmt.read::<Option<String>, _>("media_type").unwrap_or(None),
        reply_to_id: stmt
            .read::<Option<i64>, _>("reply_to_id")
            .unwrap_or(None),
        snippet: String::new(),
    }
}
