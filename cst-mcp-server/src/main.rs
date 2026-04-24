use anyhow::Result;
use cst_mcp_server::{state::ServerState, tools::CstMcpServer, watcher::start_watcher};
use rmcp::{ServiceExt, transport::stdio};
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<()> {
    let state = Arc::new(RwLock::new(ServerState::new()));

    let watcher_handle = start_watcher(Arc::clone(&state))?;

    let server = CstMcpServer::new(Arc::clone(&state), watcher_handle);

    let running = server.serve(stdio()).await?;
    running.waiting().await?;

    Ok(())
}
