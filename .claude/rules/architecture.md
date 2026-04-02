---
description: Core architecture constraints — CLI-first, JSON envelope, Tauri shells to CLI
globs: ["crates/**/*.rs", "src-tauri/**/*.rs"]
---

# Architecture Rules

1. **CLI is the single source of truth.** All business logic goes in `rss-cli` and the crates it depends on. Tauri `main.rs` only calls the CLI binary and parses JSON — no direct Rust library linking.

2. **JSON envelope protocol.** Every CLI command outputs `{"ok": true, "result": {...}}` or `{"ok": false, "error": "..."}`. New commands must follow this pattern.

3. **Internal commands use `_underscore` prefix** and `#[command(hide = true)]` in clap. They don't show in `--help`.

4. **Agent-first output.** Default output should be compact and token-efficient. `--compact` flag strips content/summary. `--json-lines` for piping.

5. **SQLite with PRAGMA foreign_keys = ON.** All foreign keys cascade on delete. Migrations use `ALTER TABLE ... ADD COLUMN` with `let _ =` for idempotency.
