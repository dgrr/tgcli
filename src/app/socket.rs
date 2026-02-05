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
}

/// Command sent from socket handler to the sync loop for actions requiring TG client
#[derive(Debug)]
pub enum SocketCommand {
    Backfill {
        chat_id: i64,
        limit: usize,
        response_tx: oneshot::Sender<Result<usize, String>>,
    },
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
}

impl SocketResponse {
    pub fn ok() -> Self {
        Self {
            ok: true,
            error: None,
            id: None,
            data: None,
            fetched: None,
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
        }
    }

    pub fn ok_with_fetched(fetched: usize) -> Self {
        Self {
            ok: true,
            error: None,
            id: None,
            data: None,
            fetched: Some(fetched),
        }
    }

    pub fn err(msg: &str) -> Self {
        Self {
            ok: false,
            error: Some(msg.to_string()),
            id: None,
            data: None,
            fetched: None,
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
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, cmd_tx).await {
                log::error!("Socket connection error: {}", e);
            }
        });
    }
}

async fn handle_connection(stream: UnixStream, cmd_tx: SocketCommandTx) -> Result<()> {
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
            Err(e) => SocketResponse::err(&format!("invalid request: {}", e)),
        };

        let json = serde_json::to_string(&resp)? + "\n";
        writer.write_all(json.as_bytes()).await?;
        writer.flush().await?;
        line.clear();
    }
    Ok(())
}
