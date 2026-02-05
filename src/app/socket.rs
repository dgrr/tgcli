use crate::store::{
    Chat, Contact, ListMessagesParams, Message, SearchMessagesParams, Store, Topic,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot};

const SOCKET_NAME: &str = "tgcli.sock";

fn socket_path(store_dir: &str) -> String {
    format!("{}/{}", store_dir, SOCKET_NAME)
}

pub fn is_socket_available(store_dir: &str) -> bool {
    let path = socket_path(store_dir);
    Path::new(&path).exists()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum SocketRequest {
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "send_text")]
    SendText { to: i64, message: String },
    #[serde(rename = "mark_read")]
    MarkRead { chat: i64, message: Option<i64> },
    #[serde(rename = "backfill")]
    Backfill {
        chat_id: i64,
        #[serde(default)]
        limit: Option<usize>,
    },
    // New RPC actions
    #[serde(rename = "clear")]
    Clear,
    #[serde(rename = "sync")]
    Sync {
        #[serde(default = "default_sync_limit")]
        limit: usize,
    },
    #[serde(rename = "chats")]
    Chats {
        #[serde(default = "default_chats_limit")]
        limit: i64,
        #[serde(default)]
        query: Option<String>,
    },
    #[serde(rename = "messages")]
    Messages {
        chat_id: i64,
        #[serde(default = "default_messages_limit")]
        limit: i64,
        #[serde(default)]
        topic_id: Option<i32>,
    },
    #[serde(rename = "search")]
    Search {
        query: String,
        #[serde(default)]
        chat_id: Option<i64>,
        #[serde(default = "default_search_limit")]
        limit: i64,
    },
    #[serde(rename = "read")]
    Read {
        chat_id: i64,
        #[serde(default)]
        topic_id: Option<i32>,
        #[serde(default)]
        all_topics: bool,
    },
    #[serde(rename = "contacts")]
    Contacts {
        #[serde(default = "default_contacts_limit")]
        limit: i64,
    },
    #[serde(rename = "topics")]
    Topics { chat_id: i64 },
    #[serde(rename = "stop")]
    Stop,
}

fn default_sync_limit() -> usize {
    100
}
fn default_chats_limit() -> i64 {
    50
}
fn default_messages_limit() -> i64 {
    50
}
fn default_search_limit() -> i64 {
    20
}
fn default_contacts_limit() -> i64 {
    50
}

/// Command sent from socket handler to the sync loop for actions requiring TG client
#[derive(Debug)]
pub enum SocketCommand {
    Backfill {
        chat_id: i64,
        limit: usize,
        response_tx: oneshot::Sender<Result<usize, String>>,
    },
    Read {
        chat_id: i64,
        topic_id: Option<i32>,
        all_topics: bool,
        response_tx: oneshot::Sender<Result<ReadResult, String>>,
    },
    Sync {
        limit: usize,
        response_tx: oneshot::Sender<Result<SyncResult, String>>,
    },
    Stop {
        response_tx: oneshot::Sender<Result<(), String>>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct ReadResult {
    pub marked_read: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topics_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub chats: u64,
    pub messages: u64,
}

pub type SocketCommandTx = mpsc::UnboundedSender<SocketCommand>;
pub type SocketCommandRx = mpsc::UnboundedReceiver<SocketCommand>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocketResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetched: Option<usize>,
    // New fields for RPC responses
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chats: Option<Vec<Chat>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<Vec<Message>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contacts: Option<Vec<Contact>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topics: Option<Vec<Topic>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleared: Option<ClearedCounts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synced: Option<SyncResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marked_read: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topics_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClearedCounts {
    pub messages: u64,
    pub chats: u64,
    pub topics: u64,
    pub contacts: u64,
}

impl SocketResponse {
    pub fn ok() -> Self {
        Self {
            ok: true,
            error: None,
            id: None,
            data: None,
            fetched: None,
            chats: None,
            messages: None,
            contacts: None,
            topics: None,
            cleared: None,
            synced: None,
            marked_read: None,
            topics_count: None,
        }
    }

    #[allow(dead_code)]
    pub fn ok_with_id(id: i64) -> Self {
        Self {
            ok: true,
            error: None,
            id: Some(id),
            data: None,
            fetched: None,
            chats: None,
            messages: None,
            contacts: None,
            topics: None,
            cleared: None,
            synced: None,
            marked_read: None,
            topics_count: None,
        }
    }

    pub fn ok_with_fetched(fetched: usize) -> Self {
        Self {
            ok: true,
            error: None,
            id: None,
            data: None,
            fetched: Some(fetched),
            chats: None,
            messages: None,
            contacts: None,
            topics: None,
            cleared: None,
            synced: None,
            marked_read: None,
            topics_count: None,
        }
    }

    pub fn ok_with_chats(chats: Vec<Chat>) -> Self {
        Self {
            ok: true,
            error: None,
            id: None,
            data: None,
            fetched: None,
            chats: Some(chats),
            messages: None,
            contacts: None,
            topics: None,
            cleared: None,
            synced: None,
            marked_read: None,
            topics_count: None,
        }
    }

    pub fn ok_with_messages(messages: Vec<Message>) -> Self {
        Self {
            ok: true,
            error: None,
            id: None,
            data: None,
            fetched: None,
            chats: None,
            messages: Some(messages),
            contacts: None,
            topics: None,
            cleared: None,
            synced: None,
            marked_read: None,
            topics_count: None,
        }
    }

    pub fn ok_with_contacts(contacts: Vec<Contact>) -> Self {
        Self {
            ok: true,
            error: None,
            id: None,
            data: None,
            fetched: None,
            chats: None,
            messages: None,
            contacts: Some(contacts),
            topics: None,
            cleared: None,
            synced: None,
            marked_read: None,
            topics_count: None,
        }
    }

    pub fn ok_with_topics(topics: Vec<Topic>) -> Self {
        Self {
            ok: true,
            error: None,
            id: None,
            data: None,
            fetched: None,
            chats: None,
            messages: None,
            contacts: None,
            topics: Some(topics),
            cleared: None,
            synced: None,
            marked_read: None,
            topics_count: None,
        }
    }

    pub fn ok_with_cleared(cleared: ClearedCounts) -> Self {
        Self {
            ok: true,
            error: None,
            id: None,
            data: None,
            fetched: None,
            chats: None,
            messages: None,
            contacts: None,
            topics: None,
            cleared: Some(cleared),
            synced: None,
            marked_read: None,
            topics_count: None,
        }
    }

    pub fn ok_with_synced(synced: SyncResult) -> Self {
        Self {
            ok: true,
            error: None,
            id: None,
            data: None,
            fetched: None,
            chats: None,
            messages: None,
            contacts: None,
            topics: None,
            cleared: None,
            synced: Some(synced),
            marked_read: None,
            topics_count: None,
        }
    }

    pub fn ok_with_read(marked_read: bool, topics_count: Option<usize>) -> Self {
        Self {
            ok: true,
            error: None,
            id: None,
            data: None,
            fetched: None,
            chats: None,
            messages: None,
            contacts: None,
            topics: None,
            cleared: None,
            synced: None,
            marked_read: Some(marked_read),
            topics_count,
        }
    }

    pub fn err(msg: &str) -> Self {
        Self {
            ok: false,
            error: Some(msg.to_string()),
            id: None,
            data: None,
            fetched: None,
            chats: None,
            messages: None,
            contacts: None,
            topics: None,
            cleared: None,
            synced: None,
            marked_read: None,
            topics_count: None,
        }
    }
}

/// Send a request to the running sync daemon via Unix socket.
pub async fn send_request(store_dir: &str, req: SocketRequest) -> Result<SocketResponse> {
    let path = socket_path(store_dir);
    let mut stream = UnixStream::connect(&path).await?;

    let json = serde_json::to_string(&req)? + "\n";
    stream.write_all(json.as_bytes()).await?;
    stream.flush().await?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await?;

    let resp: SocketResponse = serde_json::from_str(&line)?;
    Ok(resp)
}

/// Create a new socket command channel pair.
pub fn command_channel() -> (SocketCommandTx, SocketCommandRx) {
    mpsc::unbounded_channel()
}

/// Run the socket server (called from sync daemon).
/// Takes a command sender to forward requests that need access to the TG client.
pub async fn run_server(store_dir: &str, cmd_tx: SocketCommandTx) -> Result<()> {
    let path = socket_path(store_dir);
    // Remove stale socket
    let _ = std::fs::remove_file(&path);

    let listener = UnixListener::bind(&path)?;
    log::info!("Socket server listening at {}", path);

    loop {
        let (stream, _) = listener.accept().await?;
        let cmd_tx = cmd_tx.clone();
        let store_dir = store_dir.to_string();
        tokio::spawn(async move {
            // Each connection gets its own store handle for queries
            let store = match Store::open(&store_dir).await {
                Ok(s) => s,
                Err(e) => {
                    log::error!("Failed to open store for socket connection: {}", e);
                    return;
                }
            };
            if let Err(e) = handle_connection(stream, cmd_tx, store).await {
                log::error!("Socket connection error: {}", e);
            }
        });
    }
}

async fn handle_connection(
    stream: UnixStream,
    cmd_tx: SocketCommandTx,
    store: Store,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    while reader.read_line(&mut line).await? > 0 {
        let req: Result<SocketRequest, _> = serde_json::from_str(line.trim());
        let resp = match req {
            Ok(SocketRequest::Ping) => SocketResponse::ok(),

            Ok(SocketRequest::SendText { .. }) => {
                // TODO: wire this to the actual client in the sync daemon
                SocketResponse::err("send_text via socket not yet implemented in daemon")
            }

            Ok(SocketRequest::MarkRead { .. }) => {
                // TODO: wire this to the actual client in the sync daemon
                SocketResponse::err("mark_read via socket not yet implemented in daemon")
            }

            Ok(SocketRequest::Backfill { chat_id, limit }) => {
                let limit = limit.unwrap_or(100);
                let (response_tx, response_rx) = oneshot::channel();

                // Send command to sync loop
                if cmd_tx
                    .send(SocketCommand::Backfill {
                        chat_id,
                        limit,
                        response_tx,
                    })
                    .is_err()
                {
                    SocketResponse::err("sync loop not available")
                } else {
                    // Wait for response from sync loop
                    match response_rx.await {
                        Ok(Ok(fetched)) => SocketResponse::ok_with_fetched(fetched),
                        Ok(Err(e)) => SocketResponse::err(&e),
                        Err(_) => SocketResponse::err("backfill request cancelled"),
                    }
                }
            }

            // ===== NEW RPC ACTIONS =====
            Ok(SocketRequest::Clear) => {
                // Clear all tables from the store
                match clear_all(&store).await {
                    Ok(cleared) => SocketResponse::ok_with_cleared(cleared),
                    Err(e) => SocketResponse::err(&e.to_string()),
                }
            }

            Ok(SocketRequest::Sync { limit }) => {
                let (response_tx, response_rx) = oneshot::channel();

                if cmd_tx
                    .send(SocketCommand::Sync { limit, response_tx })
                    .is_err()
                {
                    SocketResponse::err("sync loop not available")
                } else {
                    match response_rx.await {
                        Ok(Ok(result)) => SocketResponse::ok_with_synced(result),
                        Ok(Err(e)) => SocketResponse::err(&e),
                        Err(_) => SocketResponse::err("sync request cancelled"),
                    }
                }
            }

            Ok(SocketRequest::Chats { limit, query }) => {
                match store.list_chats(query.as_deref(), limit).await {
                    Ok(chats) => SocketResponse::ok_with_chats(chats),
                    Err(e) => SocketResponse::err(&e.to_string()),
                }
            }

            Ok(SocketRequest::Messages {
                chat_id,
                limit,
                topic_id,
            }) => {
                let params = ListMessagesParams {
                    chat_id: Some(chat_id),
                    topic_id,
                    limit,
                    after: None,
                    before: None,
                    ignore_chats: vec![],
                    ignore_channels: false,
                };
                match store.list_messages(params).await {
                    Ok(messages) => SocketResponse::ok_with_messages(messages),
                    Err(e) => SocketResponse::err(&e.to_string()),
                }
            }

            Ok(SocketRequest::Search {
                query,
                chat_id,
                limit,
            }) => {
                let params = SearchMessagesParams {
                    query,
                    chat_id,
                    topic_id: None,
                    from_id: None,
                    limit,
                    media_type: None,
                    ignore_chats: vec![],
                    ignore_channels: false,
                };
                match store.search_messages(params).await {
                    Ok(messages) => SocketResponse::ok_with_messages(messages),
                    Err(e) => SocketResponse::err(&e.to_string()),
                }
            }

            Ok(SocketRequest::Read {
                chat_id,
                topic_id,
                all_topics,
            }) => {
                let (response_tx, response_rx) = oneshot::channel();

                if cmd_tx
                    .send(SocketCommand::Read {
                        chat_id,
                        topic_id,
                        all_topics,
                        response_tx,
                    })
                    .is_err()
                {
                    SocketResponse::err("sync loop not available")
                } else {
                    match response_rx.await {
                        Ok(Ok(result)) => {
                            SocketResponse::ok_with_read(result.marked_read, result.topics_count)
                        }
                        Ok(Err(e)) => SocketResponse::err(&e),
                        Err(_) => SocketResponse::err("read request cancelled"),
                    }
                }
            }

            Ok(SocketRequest::Contacts { limit }) => match store.list_contacts(Some(limit)).await {
                Ok(contacts) => SocketResponse::ok_with_contacts(contacts),
                Err(e) => SocketResponse::err(&e.to_string()),
            },

            Ok(SocketRequest::Topics { chat_id }) => match store.list_topics(chat_id).await {
                Ok(topics) => SocketResponse::ok_with_topics(topics),
                Err(e) => SocketResponse::err(&e.to_string()),
            },

            Ok(SocketRequest::Stop) => {
                let (response_tx, response_rx) = oneshot::channel();

                if cmd_tx.send(SocketCommand::Stop { response_tx }).is_err() {
                    SocketResponse::err("sync loop not available")
                } else {
                    match response_rx.await {
                        Ok(Ok(())) => SocketResponse::ok(),
                        Ok(Err(e)) => SocketResponse::err(&e),
                        Err(_) => SocketResponse::err("stop request cancelled"),
                    }
                }
            }

            Err(e) => SocketResponse::err(&format!("invalid request: {}", e)),
        };

        let json = serde_json::to_string(&resp)? + "\n";
        writer.write_all(json.as_bytes()).await?;
        writer.flush().await?;
        line.clear();
    }
    Ok(())
}

/// Clear all tables from the store
async fn clear_all(store: &Store) -> Result<ClearedCounts> {
    let messages = store.clear_messages().await?;
    let topics = store.clear_topics().await?;
    let chats = store.clear_chats().await?;
    let contacts = store.clear_contacts().await?;

    Ok(ClearedCounts {
        messages,
        chats,
        topics,
        contacts,
    })
}
