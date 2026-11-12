//! The browser driver interface. The engine only talks to these traits, so
//! any automation tool can be plugged in: Playwright (default), raw CDP,
//! WebDriver BiDi, or an external process speaking the same operations.

use std::time::Duration;

use async_trait::async_trait;
use enocone_spec::blocks::Locator;
use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub struct SessionOptions {
    /// Cookies and local storage from a signed-in session (driver-specific JSON).
    pub storage_state: Option<Value>,
}

#[async_trait]
pub trait Driver: Send + Sync {
    /// e.g. "playwright (chromium)".
    fn name(&self) -> String;
    /// A fresh, isolated browser session (its own cookies and storage).
    async fn new_session(&self, opts: SessionOptions) -> anyhow::Result<Box<dyn Session>>;
    async fn shutdown(&self) -> anyhow::Result<()>;
}

#[derive(Debug, Clone)]
pub enum UiAction {
    Click,
    DblClick,
    Hover,
    Fill(String),
    Check,
    Uncheck,
    Select(String),
    Press(String),
    Upload(String),
}

/// Immediate queries (no waiting); the engine polls them for assertions.
#[derive(Debug, Clone)]
pub enum UiQuery {
    Count,
    Visible,
    Text,
    Value,
    Checked,
    Enabled,
    Attribute(String),
}

#[async_trait]
pub trait Session: Send + Sync {
    async fn goto(&self, url: &str, timeout: Duration) -> anyhow::Result<()>;
    /// Perform an action, waiting up to `timeout` for the element to be actionable.
    async fn act(&self, target: &Locator, action: UiAction, timeout: Duration) -> anyhow::Result<()>;
    /// Press a key on whatever has focus.
    async fn press(&self, key: &str) -> anyhow::Result<()>;
    /// Query the first match (or the `nth`); `Value::Null` when nothing matches.
    async fn query(&self, target: &Locator, q: UiQuery) -> anyhow::Result<Value>;
    async fn url(&self) -> anyhow::Result<String>;
    async fn title(&self) -> anyhow::Result<String>;
    /// Accessibility snapshot (roles and names), for AI exploration and failure evidence.
    async fn snapshot(&self) -> anyhow::Result<String>;
    async fn screenshot(&self) -> anyhow::Result<Vec<u8>>;
    /// Evaluate a JavaScript expression (may return a promise) and return its JSON value.
    async fn eval(&self, js: &str) -> anyhow::Result<Value>;
    async fn storage_state(&self) -> anyhow::Result<Value>;
    /// Console messages since the last call (errors and warnings).
    async fn take_console(&self) -> Vec<String>;
    /// Network requests the page made since the last `clear_requests`, oldest first.
    async fn requests(&self) -> Vec<NetRequest> {
        Vec::new()
    }
    /// Forget the requests seen so far (called when a step starts).
    async fn clear_requests(&self) {}
    async fn close(&self) -> anyhow::Result<()>;
}

/// One network request a page made.
#[derive(Debug, Clone, PartialEq)]
pub struct NetRequest {
    pub method: String,
    pub url: String,
    /// The response status, when one arrived.
    pub status: Option<u16>,
    /// Finished (or failed): no longer in flight.
    pub done: bool,
}
