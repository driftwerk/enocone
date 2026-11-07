//! The enocone MCP server: lets an AI agent explore an app (browser and
//! HTTP), read and write flow documents, and run them — over stdio.
//!
//! Every exploration tool accepts the same YAML/JSON you write in a flow's
//! blocks, so what the agent tries is exactly what it will write.

pub mod app;
pub mod hub;
pub mod prototype;
mod tools;

use std::path::PathBuf;

pub use tools::Enocone;

/// The flow-format guide served to agents (`format_guide`, `enocone://guide`).
pub const GUIDE: &str = include_str!("guide.md");

/// Serve MCP over stdio for the project found at (or above) `root`.
pub async fn serve(root: PathBuf) -> anyhow::Result<()> {
    use rmcp::ServiceExt;
    let server = Enocone::new(root);
    let explorer = server.clone();
    let running = server.serve(rmcp::transport::stdio()).await?;
    let r = running.waiting().await;
    explorer.shutdown().await;
    r?;
    Ok(())
}
