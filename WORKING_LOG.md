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

### Sprint 4: Reading Experience + NVDA
- **Status**: DONE
- Keyboard: j/k articles, arrows feeds, Enter open, Esc back, Tab panes, h home, n/p unread
- Skip navigation link, aria-live announcements, article count badges
- Focus management, heading hierarchy (h1/h2/h3), aria-activedescendant
- 3px high-contrast focus rings

### Sprint 5: OPML + Search + Polish
- **Status**: DONE
- OPML import/export (zero-dependency XML parser, roundtrip test)
- Search: LIKE-based on title + content, live search bar
- High contrast mode: @media (forced-colors: active)
- Frontend: 19.8KB JS + 4.5KB CSS
- Final: 6/6 verify pass, 20/29 sprint items done (68%)

### Alignment Metrics
- Rust: 704 LOC | TS/TSX: 568 LOC | Total: ~1272 LOC
- ARIA: 31 attributes | Roles: 20 | Semantic HTML: 5
- 5 commits, all passing
