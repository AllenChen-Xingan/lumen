# Lumen

Structured feed intelligence for agents.

Lumen manages RSS feeds and exposes articles as structured JSON and clean markdown. Agents search, filter, and retrieve. Humans see the results in a desktop GUI.

## Install

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/xingan-chen/lumen/main/scripts/install.ps1 | iex
```

### Build from source

```bash
git clone https://github.com/xingan-chen/lumen.git
cd lumen
cargo build --release -p lumen
# Binary at target/release/lumen
```

For the desktop GUI:

```bash
pnpm install
cargo tauri build
```

## Quick Start

```bash
# Add a feed
lumen add https://simonwillison.net/atom/everything/

# See what you've got
lumen articles --compact --count 10

# Search
lumen search "rust OR async" --compact --since 7d

# Get full articles as markdown files
lumen fetch-full-text 401,402 --markdown
```

## How It Works

Lumen has two interfaces:

**CLI (`lumen`)** — for agents and power users. Every command outputs a JSON envelope. Compact output uses short keys (`t`, `src`, `wc`, `r`, `s`) to minimize tokens. Run `lumen` with no args to see the full self-describing schema.

**GUI (`lumen-app`)** — Tauri desktop app for browsing feeds. Keyboard-first, accessible.

### Agent workflow: scan, filter, retrieve

```
lumen search "topic" --compact    # ~50 tokens per article
  → jq filter by wc/tags/r/s     # zero tokens, local
  → lumen fetch-full-text --markdown  # writes /tmp/lumen/*.md
  → agent reads/greps files       # full text never enters context unless needed
```

### Search flags

| Flag | Example | What it does |
|------|---------|-------------|
| `--compact` | | Short keys, no content |
| `--since` | `7d`, `24h` | Relative time filter |
| `--after` | `2026-03-15` | Absolute start date |
| `--before` | `2026-03-20` | Absolute end date |
| `--on` | `2026-03-15` | Single day |
| `--feed` | `5` | Filter by feed ID |
| `--count` | `50` | Max results |
| `--sort` | `relevance` | BM25 or `date` |

### Compact output

```json
{"id":401, "t":"Title", "src":"Feed Name", "tldr":"First sentence...", "tags":"long,has_code", "wc":3200, "r":0, "s":1}
```

## Architecture

```
crates/
  rss-core/     Feed/Article structs, parser, OPML
  rss-store/    SQLite (feeds, articles, folders, FTS5)
  rss-fetch/    HTTP fetching with conditional requests
  rss-extract/  Full-text extraction, HTML→Markdown
  rss-ner/      Fact-based text feature detection
  rss-cli/      CLI binary (lumen)
src-tauri/      Tauri v2 desktop app (shells out to lumen)
src/            Solid.js frontend
```

All logic lives in the CLI. The GUI is a shell that calls `lumen` and parses JSON.

## Design Principles

**The system provides facts. Agents provide judgment.** Tags like `has_code`, `long`, `structured` are deterministic text features. The system never guesses what an article "means" to you.

**Implicit feedback only.** `is_read`, `is_starred`, and `full_content IS NOT NULL` are the engagement signals. No rating forms, no feedback tables.

**CLI is core, GUI is shell.** Every capability is available via `lumen <command>`. The desktop app adds zero logic.

## Claude Code Integration

If you use [Claude Code](https://claude.com/claude-code), Lumen ships with slash commands:

- `/lumen [topic]` — context-aware article discovery
- `/briefing` — daily unread digest

And an agent skill that teaches the scan/filter/retrieve workflow.

## License

MIT
