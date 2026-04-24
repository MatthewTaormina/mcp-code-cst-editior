use anyhow::Result;
use cst_mcp_server::{
    access::AccessConfig,
    state::ServerState,
    tools::CstMcpServer,
    watcher::start_watcher,
};
use rmcp::{ServiceExt, transport::stdio};
use std::sync::Arc;
use tokio::sync::RwLock;

// ---------------------------------------------------------------------------
// CLI argument parsing
// ---------------------------------------------------------------------------

/// Minimal argument parser — no external crate required.
///
/// Supported flags:
/// * `--workspace-path <PATH>` (required) — root of all path resolutions;
///   access to parent directories is denied by default.
/// * `--ruleset-path <PATH>` (optional) — JSON policy file that further
///   controls access with allow/deny rules.
///
/// Paths may use Unix-style separators on all platforms; the server
/// converts `/c/` prefixes to `C:\` on Windows automatically.
fn parse_args() -> Result<(String, Option<String>)> {
    let args: Vec<String> = std::env::args().collect();
    let mut workspace: Option<String> = None;
    let mut ruleset: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--workspace-path" => {
                i += 1;
                let val = args.get(i).ok_or_else(|| {
                    anyhow::anyhow!("--workspace-path requires a path argument")
                })?;
                workspace = Some(val.clone());
            }
            "--ruleset-path" => {
                i += 1;
                let val = args.get(i).ok_or_else(|| {
                    anyhow::anyhow!("--ruleset-path requires a path argument")
                })?;
                ruleset = Some(val.clone());
            }
            other => {
                anyhow::bail!("unknown argument: {other}\n\
                    Usage: cst-mcp-server --workspace-path <PATH> [--ruleset-path <PATH>]");
            }
        }
        i += 1;
    }

    let workspace = workspace.ok_or_else(|| {
        anyhow::anyhow!(
            "--workspace-path is required\n\
             Usage: cst-mcp-server --workspace-path <PATH> [--ruleset-path <PATH>]"
        )
    })?;

    Ok((workspace, ruleset))
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    let (workspace_path, ruleset_path) = parse_args()?;

    let access = Arc::new(
        AccessConfig::new(&workspace_path, ruleset_path.as_deref())
            .map_err(|e| anyhow::anyhow!("access config error: {e}"))?,
    );

    let state = Arc::new(RwLock::new(ServerState::new()));
    let watcher_handle = start_watcher(Arc::clone(&state))?;

    let server = CstMcpServer::new(Arc::clone(&state), watcher_handle, Arc::clone(&access));

    let running = server.serve(stdio()).await?;
    running.waiting().await?;

    Ok(())
}
