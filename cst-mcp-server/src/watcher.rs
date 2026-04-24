use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::RwLock;

use crate::cst::CstFile;
use crate::state::ServerState;

/// A cloneable handle to the underlying `notify` watcher.
///
/// `tools.rs` holds one of these so it can register newly tracked paths and
/// deregister untracked paths at runtime.  The `Mutex` is a *std* (blocking)
/// mutex: `Watcher::watch` / `unwatch` are synchronous calls that do not need
/// to be `await`-ed.
pub type WatcherHandle = Arc<Mutex<RecommendedWatcher>>;

/// Spin up the filesystem watcher and its tokio event-processing task.
///
/// A single `RecommendedWatcher` is created and stored behind the returned
/// `WatcherHandle`.  Callers (primarily `tools.rs`) use the handle to
/// register newly tracked paths and deregister untracked paths.
///
/// The watcher's callback bridges notify's background thread into tokio land
/// via an unbounded mpsc channel; a spawned tokio task drains that channel
/// and reloads files whose on-disk content has changed.
pub fn start_watcher(state: Arc<RwLock<ServerState>>) -> Result<WatcherHandle> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<notify::Result<Event>>();

    // Build the watcher.  The callback runs on a notify-internal thread, so
    // we only send through the channel (no async needed here).
    let watcher = RecommendedWatcher::new(
        move |event| {
            // Ignore send errors; they only occur when the server is shutting down
            // and the receiver has already been dropped.
            let _ = tx.send(event);
        },
        notify::Config::default(),
    )?;

    let handle: WatcherHandle = Arc::new(Mutex::new(watcher));

    // Clone the handle into the task so the watcher stays alive for at least
    // as long as the event loop runs.
    let handle_clone = Arc::clone(&handle);

    tokio::spawn(async move {
        // Holding this clone keeps the watcher alive even if the caller drops
        // their copy of the handle.
        let _keep_alive = handle_clone;

        while let Some(event_result) = rx.recv().await {
            match event_result {
                Ok(event) => handle_event(&state, event).await,
                Err(e) => eprintln!("watcher: notify error: {e}"),
            }
        }
    });

    Ok(handle)
}

// ---------------------------------------------------------------------------
// Helpers exposed to tools.rs for dynamic path registration
// ---------------------------------------------------------------------------

/// Register `path` with the watcher so that future on-disk modifications
/// trigger an automatic reload.
pub fn watch_path(handle: &WatcherHandle, path: &Path) -> Result<()> {
    handle
        .lock()
        .expect("watcher mutex poisoned")
        .watch(path, RecursiveMode::NonRecursive)?;
    Ok(())
}

/// Deregister `path` from the watcher.
pub fn unwatch_path(handle: &WatcherHandle, path: &Path) -> Result<()> {
    handle
        .lock()
        .expect("watcher mutex poisoned")
        .unwatch(path)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Internal event handling
// ---------------------------------------------------------------------------

async fn handle_event(state: &Arc<RwLock<ServerState>>, event: Event) {
    // Only react to modification events (covers both in-place writes and
    // close-write semantics on Linux inotify).
    if !matches!(
        event.kind,
        EventKind::Modify(_) | EventKind::Create(notify::event::CreateKind::File)
    ) {
        return;
    }

    for path in event.paths {
        reload_file(state, path).await;
    }
}

async fn reload_file(state: &Arc<RwLock<ServerState>>, path: PathBuf) {
    let mut guard = state.write().await;

    if !guard.contains(&path) {
        return;
    }

    match std::fs::read_to_string(&path) {
        Ok(content) => {
            let next_version = guard.get(&path).map_or(0, |f| f.version + 1);
            let mut file = CstFile::parse(path.clone(), &content);
            file.version = next_version;
            guard.track(path, file);
        }
        Err(e) => eprintln!("watcher: failed to reload {path:?}: {e}"),
    }
}
