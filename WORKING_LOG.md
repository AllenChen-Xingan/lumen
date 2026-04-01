# Working Log

## 2026-04-01

### Sprint 0-2: Scaffolding + Core CLI
- **Status**: DONE
- Rust workspace: rss-core (feed-rs parser), rss-store (SQLite), rss-fetch (reqwest), rss-cli (clap)
- All CLI commands working: add/list/remove/fetch/articles/read/mark-read/star
- E2E tests: 7/7 pass (real BBC RSS feed test)
- Verification scripts created: verify.sh, align.sh, e2e.sh

### Sprint 3: Tauri Bridge + Solid UI
- **Status**: DONE
- Tauri v2 backend with 7 IPC commands wrapping core library
- Solid.js three-pane layout with full ARIA landmarks
- Dark/light mode, focus-visible indicators, sr-only class
- Frontend build: 15KB JS + 3KB CSS
- Verification: 6/6 pass
