//! HTTP RPC server implementation using axum.

use crate::shutdown::ShutdownController;
use crate::store::{Chat, ListMessagesParams, Message, SearchMessagesParams, Store};
use anyhow::{Context, Result};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

/// Configuration for the RPC server.
#[derive(Debug, Clone)]
pub struct RpcServerConfig {
    /// Listen address (e.g., "127.0.0.1:5556")
    pub addr: String,
    /// Path to store directory (for accessing the database)
    pub store_dir: String,
    /// Optional PID file path
    pub pid_file: Option<PathBuf>,
    /// Request timeout
    pub request_timeout: Duration,
}

impl Default for RpcServerConfig {
    fn default() -> Self {
        Self {
            addr: "127.0.0.1:5556".to_string(),
            store_dir: "~/.tgcli".to_string(),
            pid_file: None,
            request_timeout: Duration::from_secs(30),
        }
    }
}

/// Shared state for the RPC server.
pub struct RpcState {
    /// Database store (protected by RwLock for concurrent access)
    pub store: Arc<RwLock<Store>>,
    /// Server start time
    pub start_time: Instant,
    /// Whether sync/daemon is actively running
    pub sync_running: AtomicBool,
    /// Whether Telegram client is connected
    pub tg_connected: AtomicBool,
    /// Messages received counter (from daemon updates)
    pub messages_received: AtomicU64,
    /// Messages stored counter
    pub messages_stored: AtomicU64,
    /// Store directory path
    #[allow(dead_code)]
    pub store_dir: String,
}

impl RpcState {
    pub fn new(store: Store, store_dir: String) -> Self {
        Self {
            store: Arc::new(RwLock::new(store)),
            start_time: Instant::now(),
            sync_running: AtomicBool::new(false),
            tg_connected: AtomicBool::new(false),
            messages_received: AtomicU64::new(0),
            messages_stored: AtomicU64::new(0),
            store_dir,
        }
    }

    pub fn set_sync_running(&self, running: bool) {
        self.sync_running.store(running, Ordering::Relaxed);
    }

    pub fn set_tg_connected(&self, connected: bool) {
        self.tg_connected.store(connected, Ordering::Relaxed);
    }

    pub fn increment_received(&self) {
        self.messages_received.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_stored(&self) {
        self.messages_stored.fetch_add(1, Ordering::Relaxed);
    }
}

/// The RPC server.
pub struct RpcServer {
    config: RpcServerConfig,
    state: Arc<RpcState>,
}

impl RpcServer {
    /// Create a new RPC server.
    pub async fn new(config: RpcServerConfig) -> Result<Self> {
        let store = Store::open(&config.store_dir)
            .await
            .context("Failed to open store for RPC server")?;

        let state = Arc::new(RpcState::new(store, config.store_dir.clone()));

        Ok(Self { config, state })
    }

    /// Get a clone of the shared state (for use in daemon).
    pub fn state(&self) -> Arc<RpcState> {
        Arc::clone(&self.state)
    }

    /// Start the RPC server.
    pub async fn start(self, shutdown: ShutdownController) -> Result<()> {
        // Write PID file if configured
        if let Some(ref pid_path) = self.config.pid_file {
            let pid = std::process::id();
            let mut file = fs::File::create(pid_path)
                .with_context(|| format!("Failed to create PID file: {:?}", pid_path))?;
            writeln!(file, "{}", pid)?;
            info!(pid = pid, path = ?pid_path, "PID file written");
        }

        // Build the router
        let app = self.build_router();

        let addr: SocketAddr = self
            .config
            .addr
            .parse()
            .with_context(|| format!("Invalid address: {}", self.config.addr))?;

        let listener = TcpListener::bind(addr)
            .await
            .with_context(|| format!("Failed to bind to {}", addr))?;

        info!(addr = %addr, "RPC server listening");

        let pid_file = self.config.pid_file.clone();

        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                shutdown.cancelled().await;
                info!("RPC server shutting down");
            })
            .await
            .context("RPC server error")?;

        // Cleanup PID file
        if let Some(ref pid_path) = pid_file {
            let _ = fs::remove_file(pid_path);
        }

        Ok(())
    }

    fn build_router(&self) -> Router {
        Router::new()
            // Health check
            .route("/ping", get(handle_ping))
            // Status
            .route("/status", get(handle_status))
            // Chats
            .route("/chats", get(handle_chats))
            // Messages
            .route("/messages", get(handle_messages))
            // Search
            .route("/search", post(handle_search).get(handle_search_get))
            // Send (placeholder - requires TG client integration)
            .route("/send", post(handle_send))
            // Webhook management
            .route("/webhook/get", get(handle_webhook_get))
            .route("/webhook/set", post(handle_webhook_set))
            .route("/webhook/remove", post(handle_webhook_remove))
            .route("/webhook/list", get(handle_webhook_list))
            // Middleware
            .layer(TimeoutLayer::new(self.config.request_timeout)) // TODO: migrate to with_status_code when needed
            .layer(TraceLayer::new_for_http())
            .with_state(Arc::clone(&self.state))
    }
}

// ============================================================================
// Response types
// ============================================================================

#[derive(Serialize)]
struct JsonResponse<T: Serialize> {
    ok: bool,
    #[serde(flatten)]
    data: T,
}

#[derive(Serialize)]
struct ErrorResponse {
    ok: bool,
    error: String,
}

impl<T: Serialize> JsonResponse<T> {
    fn ok(data: T) -> Json<Self> {
        Json(Self { ok: true, data })
    }
}

impl ErrorResponse {
    fn new(msg: impl Into<String>) -> (StatusCode, Json<Self>) {
        (
            StatusCode::BAD_REQUEST,
            Json(Self {
                ok: false,
                error: msg.into(),
            }),
        )
    }

    fn internal(msg: impl Into<String>) -> (StatusCode, Json<Self>) {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(Self {
                ok: false,
                error: msg.into(),
            }),
        )
    }

    #[allow(dead_code)]
    fn not_found(msg: impl Into<String>) -> (StatusCode, Json<Self>) {
        (
            StatusCode::NOT_FOUND,
            Json(Self {
                ok: false,
                error: msg.into(),
            }),
        )
    }

    fn service_unavailable(msg: impl Into<String>) -> (StatusCode, Json<Self>) {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(Self {
                ok: false,
                error: msg.into(),
            }),
        )
    }
}

// ============================================================================
// Handlers
// ============================================================================

// --- Ping ---

#[derive(Serialize)]
struct PingResponse {
    pong: bool,
}

async fn handle_ping() -> impl IntoResponse {
    JsonResponse::ok(PingResponse { pong: true })
}

// --- Status ---

#[derive(Serialize)]
struct StatusResponse {
    sync_running: bool,
    tg_connected: bool,
    chats_count: u64,
    messages_count: u64,
    messages_received: u64,
    messages_stored: u64,
    uptime_secs: u64,
    fts_enabled: bool,
}

async fn handle_status(State(state): State<Arc<RpcState>>) -> impl IntoResponse {
    let store = state.store.read().await;

    let chats_count = store.count_chats().await.unwrap_or(0);
    let messages_count = store.count_messages().await.unwrap_or(0);
    let fts_enabled = store.has_fts();

    JsonResponse::ok(StatusResponse {
        sync_running: state.sync_running.load(Ordering::Relaxed),
        tg_connected: state.tg_connected.load(Ordering::Relaxed),
        chats_count,
        messages_count,
        messages_received: state.messages_received.load(Ordering::Relaxed),
        messages_stored: state.messages_stored.load(Ordering::Relaxed),
        uptime_secs: state.start_time.elapsed().as_secs(),
        fts_enabled,
    })
}

// --- Chats ---

#[derive(Deserialize)]
struct ChatsQuery {
    query: Option<String>,
    limit: Option<i64>,
}

#[derive(Serialize)]
struct ChatsResponse {
    chats: Vec<ChatJson>,
}

#[derive(Serialize)]
struct ChatJson {
    id: i64,
    kind: String,
    name: String,
    username: Option<String>,
    last_message_ts: Option<String>,
    is_forum: bool,
}

impl From<Chat> for ChatJson {
    fn from(c: Chat) -> Self {
        Self {
            id: c.id,
            kind: c.kind,
            name: c.name,
            username: c.username,
            last_message_ts: c.last_message_ts.map(|t| t.to_rfc3339()),
            is_forum: c.is_forum,
        }
    }
}

async fn handle_chats(
    State(state): State<Arc<RpcState>>,
    Query(params): Query<ChatsQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let store = state.store.read().await;
    let limit = params.limit.unwrap_or(50);

    let chats = store
        .list_chats(params.query.as_deref(), limit)
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    Ok(JsonResponse::ok(ChatsResponse {
        chats: chats.into_iter().map(ChatJson::from).collect(),
    }))
}

// --- Messages ---

#[derive(Deserialize)]
struct MessagesQuery {
    chat_id: i64,
    topic_id: Option<i32>,
    limit: Option<i64>,
    after: Option<String>,
    before: Option<String>,
}

#[derive(Serialize)]
struct MessagesResponse {
    messages: Vec<MessageJson>,
}

#[derive(Serialize)]
struct MessageJson {
    id: i64,
    chat_id: i64,
    sender_id: i64,
    ts: String,
    edit_ts: Option<String>,
    from_me: bool,
    text: String,
    media_type: Option<String>,
    reply_to_id: Option<i64>,
    topic_id: Option<i32>,
    #[serde(skip_serializing_if = "String::is_empty")]
    snippet: String,
}

impl From<Message> for MessageJson {
    fn from(m: Message) -> Self {
        Self {
            id: m.id,
            chat_id: m.chat_id,
            sender_id: m.sender_id,
            ts: m.ts.to_rfc3339(),
            edit_ts: m.edit_ts.map(|t| t.to_rfc3339()),
            from_me: m.from_me,
            text: m.text,
            media_type: m.media_type,
            reply_to_id: m.reply_to_id,
            topic_id: m.topic_id,
            snippet: m.snippet,
        }
    }
}

async fn handle_messages(
    State(state): State<Arc<RpcState>>,
    Query(params): Query<MessagesQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let store = state.store.read().await;

    let after = params
        .after
        .as_ref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&Utc));

    let before = params
        .before
        .as_ref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&Utc));

    let messages = store
        .list_messages(ListMessagesParams {
            chat_id: Some(params.chat_id),
            topic_id: params.topic_id,
            limit: params.limit.unwrap_or(50),
            after,
            before,
            ignore_chats: vec![],
            ignore_channels: false,
        })
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    Ok(JsonResponse::ok(MessagesResponse {
        messages: messages.into_iter().map(MessageJson::from).collect(),
    }))
}

// --- Search ---

#[derive(Deserialize)]
struct SearchRequest {
    query: String,
    chat_id: Option<i64>,
    topic_id: Option<i32>,
    from_id: Option<i64>,
    limit: Option<i64>,
    media_type: Option<String>,
}

#[derive(Deserialize)]
struct SearchQueryParams {
    query: Option<String>,
    chat_id: Option<i64>,
    limit: Option<i64>,
}

#[derive(Serialize)]
struct SearchResponse {
    results: Vec<MessageJson>,
}

async fn handle_search(
    State(state): State<Arc<RpcState>>,
    Json(req): Json<SearchRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    if req.query.trim().is_empty() {
        return Err(ErrorResponse::new("query is required"));
    }

    let store = state.store.read().await;

    let results = store
        .search_messages(SearchMessagesParams {
            query: req.query,
            chat_id: req.chat_id,
            topic_id: req.topic_id,
            from_id: req.from_id,
            limit: req.limit.unwrap_or(50),
            media_type: req.media_type,
            ignore_chats: vec![],
            ignore_channels: false,
        })
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    Ok(JsonResponse::ok(SearchResponse {
        results: results.into_iter().map(MessageJson::from).collect(),
    }))
}

async fn handle_search_get(
    State(state): State<Arc<RpcState>>,
    Query(params): Query<SearchQueryParams>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let query = params.query.unwrap_or_default();
    if query.trim().is_empty() {
        return Err(ErrorResponse::new("query is required"));
    }

    let store = state.store.read().await;

    let results = store
        .search_messages(SearchMessagesParams {
            query,
            chat_id: params.chat_id,
            topic_id: None,
            from_id: None,
            limit: params.limit.unwrap_or(50),
            media_type: None,
            ignore_chats: vec![],
            ignore_channels: false,
        })
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    Ok(JsonResponse::ok(SearchResponse {
        results: results.into_iter().map(MessageJson::from).collect(),
    }))
}

// --- Send ---

#[derive(Deserialize)]
struct SendRequest {
    to: Option<i64>,
    chat_id: Option<i64>,
    message: String,
}

#[allow(dead_code)]
#[derive(Serialize)]
struct SendResponse {
    message_id: Option<i64>,
    error: Option<String>,
}

async fn handle_send(
    State(state): State<Arc<RpcState>>,
    Json(req): Json<SendRequest>,
) -> (StatusCode, Json<ErrorResponse>) {
    // Check if TG client is connected
    if !state.tg_connected.load(Ordering::Relaxed) {
        return ErrorResponse::service_unavailable(
            "Telegram not connected. Run daemon with --rpc to enable sending.",
        );
    }

    let chat_id = req.to.or(req.chat_id);
    if chat_id.is_none() {
        return ErrorResponse::new("to or chat_id is required");
    }

    if req.message.trim().is_empty() {
        return ErrorResponse::new("message is required");
    }

    // TODO: Implement actual message sending via TG client
    // This requires integrating with the App's TG client, which would need
    // to be passed into the RPC server state
    ErrorResponse::service_unavailable(
        "Sending via RPC not yet implemented. Use `tgcli send` directly.",
    )
}

// --- Webhook handlers ---

#[derive(Deserialize)]
struct WebhookGetQuery {
    chat_id: Option<i64>,
}

#[derive(Serialize)]
struct WebhookGetResponse {
    configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    chat_id: Option<i64>,
}

async fn handle_webhook_get(
    State(state): State<Arc<RpcState>>,
    Query(params): Query<WebhookGetQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let store = state.store.read().await;

    let config = store
        .get_webhook()
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    match config {
        Some(cfg) => {
            // If chat_id filter is specified, check if it matches
            if let Some(requested_chat) = params.chat_id {
                if let Some(cfg_chat) = cfg.chat_id {
                    if cfg_chat != requested_chat {
                        return Ok(JsonResponse::ok(WebhookGetResponse {
                            configured: false,
                            url: None,
                            prompt: None,
                            chat_id: None,
                        }));
                    }
                }
            }

            Ok(JsonResponse::ok(WebhookGetResponse {
                configured: true,
                url: Some(cfg.url),
                prompt: Some(cfg.prompt),
                chat_id: cfg.chat_id,
            }))
        }
        None => Ok(JsonResponse::ok(WebhookGetResponse {
            configured: false,
            url: None,
            prompt: None,
            chat_id: None,
        })),
    }
}

#[derive(Deserialize)]
struct WebhookSetRequest {
    url: String,
    prompt: String,
    chat_id: Option<i64>,
}

#[derive(Serialize)]
struct WebhookSetResponse {
    url: String,
    prompt: String,
    chat_id: Option<i64>,
}

async fn handle_webhook_set(
    State(state): State<Arc<RpcState>>,
    Json(req): Json<WebhookSetRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    if req.url.trim().is_empty() {
        return Err(ErrorResponse::new("url is required"));
    }

    let store = state.store.read().await;

    store
        .set_webhook(&req.url, &req.prompt, req.chat_id)
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    info!(url = %req.url, chat_id = ?req.chat_id, "Webhook configured via RPC");

    Ok(JsonResponse::ok(WebhookSetResponse {
        url: req.url,
        prompt: req.prompt,
        chat_id: req.chat_id,
    }))
}

#[derive(Deserialize)]
struct WebhookRemoveRequest {
    #[allow(dead_code)]
    chat_id: Option<i64>,
}

#[derive(Serialize)]
struct WebhookRemoveResponse {
    removed: bool,
}

async fn handle_webhook_remove(
    State(state): State<Arc<RpcState>>,
    Json(_req): Json<WebhookRemoveRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let store = state.store.read().await;

    // Note: Current tgcli webhook implementation only supports a single webhook
    // (not per-chat like wacli). For full parity, we'd need to extend the schema.
    let removed = store
        .remove_webhook()
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    if removed {
        info!("Webhook removed via RPC");
    }

    Ok(JsonResponse::ok(WebhookRemoveResponse { removed }))
}

#[derive(Serialize)]
struct WebhookListResponse {
    webhooks: Vec<WebhookJson>,
    count: usize,
}

#[derive(Serialize)]
struct WebhookJson {
    url: String,
    prompt: String,
    chat_id: Option<i64>,
    is_catch_all: bool,
}

async fn handle_webhook_list(
    State(state): State<Arc<RpcState>>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let store = state.store.read().await;

    let config = store
        .get_webhook()
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    let webhooks: Vec<WebhookJson> = match config {
        Some(cfg) => vec![WebhookJson {
            url: cfg.url,
            prompt: cfg.prompt,
            is_catch_all: cfg.chat_id.is_none(),
            chat_id: cfg.chat_id,
        }],
        None => vec![],
    };

    let count = webhooks.len();

    Ok(JsonResponse::ok(WebhookListResponse { webhooks, count }))
}
