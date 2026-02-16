//! Production-ready HTTP RPC server for tgcli.
//!
//! Provides a REST API for querying chats, messages, and managing webhooks
//! while the daemon runs. Mirrors wacli's RPC implementation for consistency.
//!
//! ## Endpoints
//!
//! - `GET  /ping`           - Health check
//! - `GET  /status`         - Server status (uptime, counts, sync state)
//! - `GET  /chats`          - List chats (query, limit params)
//! - `GET  /messages`       - Get messages (chat_id required, limit/after/before params)
//! - `POST /search`         - Search messages (query in body or params)
//! - `POST /send`           - Send a message (requires active TG connection)
//! - `GET  /webhook/get`    - Get webhook config for a chat
//! - `POST /webhook/set`    - Set webhook for a chat
//! - `POST /webhook/remove` - Remove webhook for a chat
//! - `GET  /webhook/list`   - List all configured webhooks

mod server;

pub use server::{RpcServer, RpcServerConfig, RpcState};
