# CST-MCP Server

A [Model Context Protocol](https://modelcontextprotocol.io) (MCP) server that lets an AI assistant
edit source files through their **Concrete Syntax Tree** (CST) rather than raw text.

Files are parsed into [rowan](https://github.com/rust-analyzer/rowan) red-green trees and held in
memory.  A filesystem watcher (`notify`) automatically reloads changed files.  Every mutation
carries a version counter so the server can detect and reject stale edits.

---

## Features

| Capability | Description |
|---|---|
| **Line-level editing** | Read and replace individual lines while preserving all other content verbatim (lossless round-trip) |
| **Token-level inspection** | Rust files (`.rs`) are lexed into semantic tokens — Keywords, Identifiers, Literals, Comments, Whitespace, Punctuation — visible at the sub-line level |
| **Conflict detection** | Optimistic locking via `expected_version` guards against clobbering watcher-triggered reloads |
| **Auto-reload** | inotify-backed watcher detects external changes and reloads tracked files without dropping sessions |
| **MCP stdio transport** | Works out-of-the-box with Claude Desktop, Claude Code, and any MCP-compliant client |

---

## Installation

### Prerequisites

- Rust 1.70+ (stable)
- Linux, macOS, or Windows (inotify on Linux, FSEvents on macOS, ReadDirectoryChanges on Windows)

### Build

```bash
git clone https://github.com/MatthewTaormina/mcp-code-cst-editior.git
cd mcp-code-cst-editior
cargo build --release
# binary is at: target/release/cst-mcp-server
```

### inotify limits (Linux only)

On Debian/Ubuntu/Mint the default kernel inotify limits are very low.  Run the
bundled setup script once to raise them:

```bash
sudo scripts/setup-inotify.sh
```

This writes `/etc/sysctl.d/99-cst-mcp-inotify.conf` (persists across reboots).

---

## Claude Desktop configuration

Add the server to `~/Library/Application Support/Claude/claude_desktop_config.json`
(macOS) or `%APPDATA%\Claude\claude_desktop_config.json` (Windows):

```jsonc
{
  "mcpServers": {
    "cst-editor": {
      "command": "/absolute/path/to/cst-mcp-server"
    }
  }
}
```

---

## Tools reference

All tools communicate over **stdio** using the MCP JSON-RPC protocol.  Text
responses that begin with `"ok:"` indicate success; responses beginning with
`"error:"` indicate failure; responses beginning with `"conflict:"` indicate
an optimistic-lock violation.

### `track_file`

Load a file from disk into memory and begin watching it for external changes.

| Parameter | Type | Description |
|---|---|---|
| `path` | `string` | Absolute path to the file |

**Returns:** `"ok: tracking <path>"` or `"error: …"`

---

### `untrack_file`

Remove a file from memory and stop watching it.

| Parameter | Type | Description |
|---|---|---|
| `path` | `string` | Absolute path of the tracked file |

**Returns:** `"ok: untracked <path>"` or `"error: …"`

---

### `list_tracked_files`

List every file currently held in memory.

*(No parameters.)*

**Returns:** JSON

```json
{
  "count": 2,
  "files": ["/abs/path/a.rs", "/abs/path/b.rs"]
}
```

Paths are sorted lexicographically.

---

### `load_file`

Return a one-line summary of a tracked file's CST (line count + version).

| Parameter | Type | Description |
|---|---|---|
| `path` | `string` | Absolute path of the tracked file |

**Returns:** `"ok: <path> — N lines, CST version V"`

---

### `get_tree_skeleton`

List all Line nodes in a tracked file with their IDs, byte spans, and
text previews.  Use this to navigate the file before calling `get_node` or
`edit_node`.

| Parameter | Type | Description |
|---|---|---|
| `path` | `string` | Absolute path of the tracked file |

**Returns:** JSON

```json
{
  "version": 0,
  "nodes": [
    { "node_id": 0, "kind": "Line", "text_preview": "fn main() {", "span": {"start": 0, "end": 13} },
    { "node_id": 1, "kind": "Line", "text_preview": "    println!(\"hello\");", "span": {"start": 13, "end": 36} },
    { "node_id": 2, "kind": "Line", "text_preview": "}", "span": {"start": 36, "end": 38} }
  ]
}
```

---

### `get_node`

Return full metadata for a single Line node.

| Parameter | Type | Description |
|---|---|---|
| `path` | `string` | Absolute path of the tracked file |
| `node_id` | `integer` | 0-based line index |

**Returns:** JSON

```json
{
  "node_id": 1,
  "kind": "Line",
  "text": "    println!(\"hello\");\n",
  "span": {"start": 13, "end": 36},
  "version": 0
}
```

Use the `version` field with `edit_node`'s `expected_version` parameter.

---

### `get_line_tokens`

Return the token-level children of a single Line node.

For `.rs` files each token has a semantic `kind`:

| Kind | Example |
|---|---|
| `Keyword` | `fn`, `let`, `pub`, `match` |
| `Identifier` | `main`, `println`, `x` |
| `Literal` | `42`, `"hello"`, `3.14`, `b"bytes"` |
| `Comment` | `// note`, `/* block */` |
| `Whitespace` | `    ` (spaces / tabs) |
| `Newline` | `\n` |
| `Punctuation` | `(`, `)`, `{`, `->`, `;` |

For all other file types each line is a single `Text`-classified token.

| Parameter | Type | Description |
|---|---|---|
| `path` | `string` | Absolute path of the tracked file |
| `node_id` | `integer` | 0-based line index |

**Returns:** JSON

```json
{
  "line_node_id": 0,
  "language": "Rust",
  "tokens": [
    { "token_idx": 0, "kind": "Keyword",     "text": "fn",   "span": {"start": 0, "end": 2} },
    { "token_idx": 1, "kind": "Whitespace",  "text": " ",    "span": {"start": 2, "end": 3} },
    { "token_idx": 2, "kind": "Identifier",  "text": "main", "span": {"start": 3, "end": 7} }
  ],
  "version": 0
}
```

---

### `edit_node`

Replace the content of one Line node.  All other lines are preserved verbatim.

| Parameter | Type | Description |
|---|---|---|
| `path` | `string` | Absolute path of the tracked file |
| `node_id` | `integer` | 0-based line index |
| `new_text` | `string` | Replacement text (trailing newline is preserved from the original line) |
| `expected_version` | `integer?` | *Optional.* If provided, the edit is rejected when the file's actual version differs (conflict guard) |

**Returns:**
- `"ok: node N in <path> updated (CST version V)"` on success
- `"conflict: <path> is at version A but expected version B — …"` on stale-read

**Recommended workflow:**

```
get_node(path, node_id)     → {version: V, text: "…"}
edit_node(path, node_id, new_text, expected_version=V)
save_file(path)
```

---

### `insert_lines`

Insert one or more new lines at a position in a tracked file's CST.

All existing lines are preserved verbatim.  A trailing `\n` is auto-appended
to any line that lacks one.  Uses rowan's native `splice_children` — unchanged
Line nodes are shared without re-parsing.

| Parameter | Type | Description |
|---|---|---|
| `path` | `string` | Absolute path of the tracked file |
| `insert_after` | `integer \| null` | Line index after which to insert. `null` (or omit) to prepend at the start |
| `lines` | `string[]` | One or more new line strings to insert |
| `expected_version` | `integer?` | *Optional.* Conflict guard (same semantics as `edit_node`) |

**Returns:** JSON on success, `"conflict: …"` or `"error: …"` on failure.

```json
{
  "inserted_count": 1,
  "first_node_id": 1,
  "version": 2
}
```

**Example — insert a comment between lines 0 and 1:**

```
insert_lines(path, insert_after=0, lines=["    // inserted comment\n"], expected_version=V)
```

---

### `delete_lines`

Delete one or more consecutive Line nodes from a tracked file's CST.

All remaining lines are preserved verbatim.  Deleted node IDs are compacted —
the node that was at `node_id + count` becomes `node_id` after the call.  Uses
rowan's native `splice_children`.

| Parameter | Type | Description |
|---|---|---|
| `path` | `string` | Absolute path of the tracked file |
| `node_id` | `integer` | 0-based index of the first line to delete |
| `count` | `integer` | Number of consecutive lines to remove (≥ 1) |
| `expected_version` | `integer?` | *Optional.* Conflict guard (same semantics as `edit_node`) |

**Returns:** `"ok: deleted N line(s) …"` on success, `"conflict: …"` or `"error: …"` on failure.

---

### `save_file`

Flush the in-memory CST back to disk (lossless round-trip).

| Parameter | Type | Description |
|---|---|---|
| `path` | `string` | Absolute path of the tracked file |

**Returns:** `"ok: saved <path> (CST version V)"` or `"error: …"`

---

## Architecture

```
main.rs          — tokio entry point; wires state + watcher + MCP stdio
src/
  cst.rs         — rowan CST (CstFile, NodeInfo, TokenInfo, FileLanguage)
  lexer.rs       — lossless Rust token lexer; plain-text fallback
  state.rs       — ServerState: versioned HashMap<PathBuf, CstFile>
  watcher.rs     — notify watcher; auto-reloads tracked files on disk change
  tools.rs       — 11 MCP tools exposed via #[tool_router(server_handler)]
tests/
  integration.rs — 29 integration tests (watcher, conflict, token-level, concurrent)
scripts/
  setup-inotify.sh — raise inotify limits on Debian/Mint (run once, sudo)
```

## Testing

```bash
cargo test          # run all 88 tests (unit + integration)
cargo build         # verify binary compiles
```
