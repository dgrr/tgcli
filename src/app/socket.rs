use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

const SOCKET_NAME: &str = "tgrs.sock";

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
    MarkRead {
        chat: i64,
        message: Option<i64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocketResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
}

impl SocketResponse {
    pub fn ok() -> Self {
        Self {
            ok: true,
            error: None,
            id: None,
            data: None,
        }
    }

    pub fn ok_with_id(id: i64) -> Self {
        Self {
            ok: true,
            error: None,
            id: Some(id),
            data: None,
        }
    }

    pub fn err(msg: &str) -> Self {
        Self {
            ok: false,
            error: Some(msg.to_string()),
            id: None,
            data: None,
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

/// Run the socket server (called from sync daemon).
pub async fn run_server(store_dir: &str) -> Result<()> {
    let path = socket_path(store_dir);
    // Remove stale socket
    let _ = std::fs::remove_file(&path);

    let listener = UnixListener::bind(&path)?;
    log::info!("Socket server listening at {}", path);

    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream).await {
                log::error!("Socket connection error: {}", e);
            }
        });
    }
}

async fn handle_connection(stream: UnixStream) -> Result<()> {
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
            Err(e) => SocketResponse::err(&format!("invalid request: {}", e)),
        };

        let json = serde_json::to_string(&resp)? + "\n";
        writer.write_all(json.as_bytes()).await?;
        writer.flush().await?;
        line.clear();
    }
    Ok(())
}
