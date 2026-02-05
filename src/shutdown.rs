//! Graceful shutdown coordination using cancellation tokens.
//!
//! This module provides a centralized way to coordinate shutdown across
//! async tasks. When Ctrl+C is pressed, all tasks watching the shutdown
//! signal will be notified to stop gracefully.

use std::sync::OnceLock;
use tokio_util::sync::CancellationToken;

/// Global shutdown controller instance
static GLOBAL_SHUTDOWN: OnceLock<ShutdownController> = OnceLock::new();

/// Controller for coordinating graceful shutdown across tasks.
#[derive(Clone)]
pub struct ShutdownController {
    token: CancellationToken,
}

impl ShutdownController {
    /// Create a new shutdown controller.
    pub fn new() -> Self {
        Self {
            token: CancellationToken::new(),
        }
    }

    /// Trigger shutdown, notifying all waiting tasks.
    pub fn trigger(&self) {
        self.token.cancel();
    }

    /// Check if shutdown has been triggered.
    pub fn is_triggered(&self) -> bool {
        self.token.is_cancelled()
    }

    /// Get a future that completes when shutdown is triggered.
    /// Use this in `tokio::select!` to check for shutdown.
    pub async fn cancelled(&self) {
        self.token.cancelled().await
    }

    /// Get the underlying cancellation token for advanced use cases.
    #[allow(dead_code)]
    pub fn token(&self) -> &CancellationToken {
        &self.token
    }

    /// Create a child token that will be cancelled when this controller
    /// is triggered, but can also be cancelled independently.
    pub fn child_token(&self) -> CancellationToken {
        self.token.child_token()
    }
}

impl Default for ShutdownController {
    fn default() -> Self {
        Self::new()
    }
}

/// Set the global shutdown controller.
/// Should be called once at startup.
pub fn set_global(controller: ShutdownController) {
    let _ = GLOBAL_SHUTDOWN.set(controller);
}

/// Get the global shutdown controller.
/// Returns a default (never-triggered) controller if not set.
pub fn global() -> ShutdownController {
    GLOBAL_SHUTDOWN
        .get()
        .cloned()
        .unwrap_or_else(ShutdownController::new)
}

/// Check if global shutdown has been triggered.
#[allow(dead_code)]
pub fn is_shutdown() -> bool {
    GLOBAL_SHUTDOWN
        .get()
        .map(|c| c.is_triggered())
        .unwrap_or(false)
}
