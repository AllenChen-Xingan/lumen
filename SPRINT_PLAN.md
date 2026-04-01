# RSS Reader - Sprint Development Plan

## Vision
A minimalist RSS reader for Windows, guided by Paul Graham's "Taste for Makers" principles,
fully accessible with NVDA screen reader. Architecture: Rust CLI core + Tauri + Solid.js frontend.

## Sprint Map

### Sprint 0: Scaffolding [CURRENT]
- [x] Git init
- [ ] Rust workspace (rss-core, rss-store, rss-fetch, rss-cli)
- [ ] Tauri project
- [ ] Solid.js frontend skeleton
- [ ] Verifier script
- [ ] Working log system

### Sprint 1: Feed Parsing + Storage
- [ ] feed-rs based RSS/Atom parser in rss-core
- [ ] SQLite schema (feeds, articles) in rss-store
- [ ] CLI: `add <url>`, `list`, `remove <id>`
- [ ] E2E test: add feed -> list shows it -> remove works

### Sprint 2: Fetch + Article Pipeline
- [ ] HTTP fetcher with reqwest in rss-fetch
- [ ] Article storage pipeline
- [ ] CLI: `fetch`, `articles`, `read <id>`, `mark-read <id>`, `star <id>`
- [ ] E2E test: fetch real RSS feed -> articles stored -> readable

### Sprint 3: Tauri Bridge + Minimal UI
- [ ] Tauri IPC commands wrapping core functions
- [ ] Three-pane Solid.js layout
- [ ] ARIA landmarks (nav, complementary, main)
- [ ] E2E test: app launches -> feeds display -> articles render

### Sprint 4: Reading Experience + NVDA
- [ ] Article HTML rendering
- [ ] Read/unread/star state management
- [ ] Keyboard navigation (j/k/Enter/Esc/Tab)
- [ ] ARIA live regions for state changes
- [ ] Focus management across panes
- [ ] E2E test: keyboard-only full workflow + ARIA audit

### Sprint 5: Polish
- [ ] OPML import/export
- [ ] Full-text search
- [ ] Auto-refresh interval
- [ ] High contrast theme
- [ ] Final NVDA manual test checklist

## Verification Gates
Each sprint must pass `scripts/verify.sh` before merge.
