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
| **Structural mutations** | Insert and delete lines using rowan's native `splice_children` — no re-parse of unchanged nodes |
| **Token-level inspection** | Rust files (`.rs`) are lexed into semantic tokens — Keywords, Identifiers, Literals, Comments, Whitespace, Punctuation — visible at the sub-line level |
| **Conflict detection** | Optimistic locking via `expected_version` guards against clobbering watcher-triggered reloads |
| **Auto-reload** | inotify-backed watcher detects external changes and reloads tracked files without dropping sessions |
| **Workspace confinement** | `--workspace-path` is required at startup; all paths are resolved relative to the workspace root and escape attempts (`../../`) are rejected |
| **JSON ruleset** | Optional `--ruleset-path` points to a policy document that controls access by action, resource glob, and priority |
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

## Startup

`--workspace-path` is **required**.  All paths supplied to any tool are
resolved against this root.  Paths that escape the workspace (e.g. via `../`)
are rejected with an `"error:"` response.

```bash
# Minimal — default-allow everything inside the workspace
cst-mcp-server --workspace-path /home/alice/myproject

# With a ruleset that further restricts access
cst-mcp-server --workspace-path /home/alice/myproject \
               --ruleset-path  /home/alice/myproject/.cst-rules.json
```

### Path format

All paths may use Unix-style forward slashes on every platform.  On Windows
the prefix `/c/` is converted to `C:\` automatically, so callers always send
Unix paths regardless of the host OS.

| Client sends | Resolved on Windows | Resolved on Linux/macOS |
|---|---|---|
| `/home/alice/src/main.rs` | (unchanged, treated as absolute) | `/home/alice/src/main.rs` |
| `/c/Users/alice/src/main.rs` | `C:\Users\alice\src\main.rs` | `/c/Users/alice/src/main.rs` |
| `src/main.rs` | `<workspace>\src\main.rs` | `<workspace>/src/main.rs` |

---

## JSON Ruleset

The optional `--ruleset-path` file controls fine-grained access with ordered
allow/deny rules.  Rules are relative to the workspace root unless the
resource pattern starts with `/` (absolute).

### Schema

```json
{
  "rules": [
    {
      "effect":   "deny" | "allow",
      "priority": <integer — higher wins>,
      "actions":  ["track" | "untrack" | "load" | "read" | "edit" | "insert" | "delete" | "save" | "*"],
      "resources": ["<glob-pattern>" | ...]
    }
  ]
}
```

### Action names

| Action | Triggered by |
|---|---|
| `track` | `track_file` |
| `untrack` | `untrack_file` |
| `load` | `load_file` |
| `read` | `get_node`, `get_tree_skeleton`, `get_line_tokens` |
| `edit` | `edit_node` |
| `insert` | `insert_lines` |
| `delete` | `delete_lines` |
| `save` | `save_file` |
| `query` | `query_file` |
| `*` | any action |

### Resource glob patterns

| Pattern | Matches |
|---|---|
| `src/*.rs` | All `.rs` files directly in `src/` |
| `src/**/*.rs` | All `.rs` files anywhere under `src/` |
| `*.lock` | All `.lock` files in the workspace root |
| `**` | Every file in the workspace |
| `/etc/**` | Absolute pattern — every file under `/etc/` |

### Rule evaluation

1. Rules are sorted by **descending priority** (highest number wins).
2. The **first matching** rule (action + resource) determines the outcome.
3. If no rule matches, the default is **allow** (the workspace-containment
   check is the primary security boundary).
4. When two rules have equal priority, **deny** wins.

### Examples

```json
{
  "rules": [
    {
      "comment": "Protect lockfiles from any modification",
      "effect": "deny",
      "priority": 200,
      "actions": ["edit", "insert", "delete", "save"],
      "resources": ["*.lock", "**/Cargo.lock"]
    },
    {
      "comment": "Allow read access to the entire workspace",
      "effect": "allow",
      "priority": 50,
      "actions": ["read", "load", "track"],
      "resources": ["**"]
    },
    {
      "comment": "Deny all by default (lower priority — override above)",
      "effect": "deny",
      "priority": 10,
      "actions": ["*"],
      "resources": ["**"]
    }
  ]
}
```

---

## Claude Desktop configuration

Add the server to `~/Library/Application Support/Claude/claude_desktop_config.json`
(macOS) or `%APPDATA%\Claude\claude_desktop_config.json` (Windows):

```jsonc
{
  "mcpServers": {
    "cst-editor": {
      "command": "/absolute/path/to/cst-mcp-server",
      "args": ["--workspace-path", "/home/alice/myproject"]
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

### `query_file`

Search the CST of a single tracked file using any combination of text
filters, semantic patterns, identifier search, and scope-depth constraints.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `path` | `string` | ✓ | Path of the tracked file |
| `query` | `QueryExpr` | ✓ | Query expression (see below) |

#### QueryExpr fields

| Field | Type | Description |
|---|---|---|
| `kind` | `string` | Filter by kind name (case-insensitive).  Line-depth: `"Line"`.  Token-depth (.rs): `Keyword` \| `Identifier` \| `Literal` \| `Comment` \| `Whitespace` \| `Punctuation` \| `Newline`. |
| `text_contains` | `string` | Substring match (case-sensitive). |
| `text_glob` | `string` | Glob pattern — `*` = any chars, `?` = one char (no path-separator semantics). |
| `node_id_from` | `integer` | First line to consider (0-based, inclusive). |
| `node_id_to` | `integer` | Last line to consider (0-based, inclusive). |
| `depth` | `"line"` \| `"token"` | Match at line level (default) or token level.  Overridden by `semantic` / `identifier_name`. |
| `semantic` | `string` | Named syntactic construct (see table below). Returns a `capture` per match. |
| `identifier_name` | `string` | Find every token-level occurrence of this exact identifier. |
| `scope_depth_min` | `integer` | Only lines where brace-nesting depth ≥ N (0 = top-level). |
| `scope_depth_max` | `integer` | Only lines where brace-nesting depth ≤ N. |

#### Semantic pattern values

Scope depth is computed by counting `{` / `}` Punctuation tokens (never
generated inside string literals or comments, so typically accurate).
Multi-line string literals are a known limitation.

| `semantic` value | Matches | `capture` |
|---|---|---|
| `fn_def` | `fn <name>(…)` | function name |
| `struct_def` | `struct <name>` | struct name |
| `enum_def` | `enum <name>` | enum name |
| `trait_def` | `trait <name>` | trait name |
| `impl_block` | `impl [Trait for] Type` | first identifier after `impl` |
| `type_def` | `type <name> =` | alias name |
| `variable_def` | `let [mut] <name>` / `const <name>` / `static [mut] <name>` | variable name |
| `use_stmt` | `use <path>;` | the imported path |
| `macro_call` | `<name>!(…)` | macro name |

#### Examples

```json
// All top-level function definitions
{"semantic": "fn_def", "scope_depth_max": 0}

// Every occurrence of the identifier "conn"
{"identifier_name": "conn"}

// Lines containing "TODO" anywhere in the file
{"text_contains": "TODO"}

// All keyword tokens in the first 10 lines, token-depth
{"depth": "token", "kind": "Keyword", "node_id_to": 9}

// All variable definitions inside exactly one level of nesting
{"semantic": "variable_def", "scope_depth_min": 1, "scope_depth_max": 1}
```

**Returns:** JSON

```json
{
  "file": "/abs/path/src/main.rs",
  "language": "Rust",
  "version": 1,
  "match_count": 2,
  "matches": [
    {
      "node_id": 0,
      "kind": "Line",
      "text": "fn main() {\n",
      "span_start": 0,
      "span_end": 12,
      "scope_depth": 0,
      "capture": "main"
    }
  ]
}
```

Token-depth matches also include `"token_idx"`.  Semantic matches include
`"capture"`.  Both fields are omitted when not applicable.

---

### `query_workspace`

Run the same `QueryExpr` across **all tracked files** simultaneously.  Files
with zero matches are omitted from the output.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `query` | `QueryExpr` | ✓ | Same expression as `query_file` |
| `graph` | `GraphQuery \| null` | — | **Reserved — pass `null` or omit.**  Future releases will use this field for cross-file relationship queries (import graphs, call graphs, reference chains). |

**Returns:** JSON

```json
{
  "total_files_searched": 3,
  "files_with_matches": 1,
  "total_matches": 4,
  "results": [
    {
      "file": "/abs/path/src/main.rs",
      "language": "Rust",
      "version": 0,
      "match_count": 4,
      "matches": [ … ]
    }
  ]
}
```

> **Planned:** The `graph` field will later accept a `GraphQuery` object to
> express relationship-aware searches such as *"all files that import symbol X"*,
> *"callers of function Y"*, or *"modules reachable from the entry point"*.  The
> schema will be documented when the feature is implemented.

---

### `query_tool`

Return documentation and tool-selection guidance for this server.

| Parameter | Type | Description |
|---|---|---|
| `tool_name` | `string` (optional) | Name of the tool to look up.  Omit for the full catalog + selection guide. |

Omit `tool_name` to get a categorised tool catalog (tracking / inspection /
query / editing / help) plus a typical workflow.  Provide a specific tool name
to get focused parameter docs and examples.

**Returns:** JSON catalog or single-tool entry.

---

## Architecture

```
main.rs          — tokio entry point; parses --workspace-path / --ruleset-path; wires access + state + watcher + MCP stdio
src/
  access.rs      — AccessConfig: workspace confinement, JSON ruleset loading, glob-based rule evaluation
  cst.rs         — rowan CST (CstFile, NodeInfo, TokenInfo, QueryExpr, QueryMatch, FileLanguage)
  lexer.rs       — lossless Rust token lexer; plain-text fallback
  state.rs       — ServerState: versioned HashMap<PathBuf, CstFile>
  watcher.rs     — notify watcher; auto-reloads tracked files on disk change
  tools.rs       — 14 MCP tools exposed via #[tool_router(server_handler)]
tests/
  integration.rs — 29 integration tests (watcher, conflict, token-level, concurrent)
scripts/
  setup-inotify.sh — raise inotify limits on Debian/Mint (run once, sudo)
```

## Testing

```bash
cargo test          # run all 125 tests (unit + integration)
cargo build         # verify binary compiles
```
