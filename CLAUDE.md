# RSS Feed Manager — Agent-Native Information System

## What This Is

Not an RSS reader. An **information management system** where RSS feeds are "informants" — agents subscribe, search, classify, and retrieve; humans see the results in a GUI shell.

**Architecture**: Rust CLI core → Tauri IPC bridge → Solid.js frontend

## Project Structure

```
crates/
  rss-core/     Feed/Article structs, feed-rs parser, OPML
  rss-store/    SQLite storage (feeds, articles, folders, entities)
  rss-fetch/    HTTP fetching (reqwest)
  rss-extract/  Content extraction
  rss-ner/      Fact-based text feature detection (NOT AI classification)
  rss-cli/      clap CLI — all output is JSON envelopes {ok, result/error}
src-tauri/      Tauri v2 IPC — shells out to rss-cli binary
src/            Solid.js frontend (App.tsx is the entire UI)
scripts/        verify.sh, align.sh
tests/          e2e.sh, a11y-audit.mjs, nvda-checklist.sh
```

## Key Design Decisions

### 1. CLI is core, GUI is shell
All logic lives in `rss-cli`. Tauri `main.rs` calls the CLI binary via `std::process::Command` and parses JSON output. No Rust library linking between crates and Tauri.

### 2. Fact-based annotation, NOT AI classification
`rss-ner` does deterministic text feature detection (length, has_code, has_steps, has_images). Smart folders are fact-query views (unread/long/tutorial/recent). **Never add "AI guesses user intent" features.** High-order intentionality judgments belong to the user/agent, not the system.

### 3. Implicit feedback only
`is_read`, `is_starred`, `full_content IS NOT NULL` are the engagement signals. No explicit feedback forms, tag correction UI, or feedback tables. If a feature's primary purpose is collecting feedback, delete it.

### 4. Agent information retrieval: coarse to fine
- **Scan**: `--compact` flag → id, title, tldr, tags, url (saves tokens)
- **Browse**: default output with content/summary
- **Deep**: `fetch-full-text <id>` on demand
- Never send full articles to agents by default

### 5. JSON envelope protocol
Every CLI command outputs `{"ok": true, "result": {...}}` or `{"ok": false, "error": "..."}`. Internal commands use `_underscore` prefix and are hidden from help.

## Development

```bash
# Build everything
cargo build

# Run CLI
cargo run -p rss-cli -- <command>

# Run Tauri dev
pnpm dev          # frontend dev server on :3000
cargo tauri dev   # full app

# Tests
bash tests/e2e.sh           # E2E against real feeds
bash scripts/verify.sh      # build + lint checks
node tests/a11y-audit.mjs   # accessibility audit
```

## SQLite Schema (key tables)

- `feeds` — id, title, url, site_url, description, added_at, folder_id
- `articles` — id, feed_id, guid, title, url, content, summary, published_at, is_read, is_starred, fetched_at, full_content, tldr, tags, analyzed
- `folders` — id, name, folder_type (manual/smart), query
- `folder_feeds` — folder_id, feed_id (M:N)
- `entities` — id, name, entity_type, article_id, context, score

PRAGMA foreign_keys = ON. Cascade delete on feed removal.

## What NOT to Do

- Don't add AI/ML classification for article categorization
- Don't add explicit feedback mechanisms (forms, rating UI, correction commands)
- Don't send full article content to agents unless explicitly requested
- Don't add features whose primary purpose is "collecting data about the user"
- Don't bypass the CLI→JSON envelope pattern from Tauri
