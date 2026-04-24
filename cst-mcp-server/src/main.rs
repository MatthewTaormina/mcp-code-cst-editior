mod cst;
mod state;
mod tools;
mod watcher;

use anyhow::Result;
use rmcp::{ServiceExt, transport::stdio};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::state::ServerState;
use crate::tools::CstMcpServer;
use crate::watcher::start_watcher;

#[tokio::main]
async fn main() -> Result<()> {
    let state = Arc::new(RwLock::new(ServerState::new()));

    let watcher_handle = start_watcher(Arc::clone(&state))?;

    let server = CstMcpServer::new(Arc::clone(&state), watcher_handle);

    let running = server.serve(stdio()).await?;
    running.waiting().await?;

    Ok(())
}
