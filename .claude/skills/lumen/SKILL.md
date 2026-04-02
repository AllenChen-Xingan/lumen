---
name: lumen
description: Search and retrieve articles from RSS feeds via Lumen CLI. Use when the user asks to find articles, research a topic from feeds, get a daily briefing, or read specific articles. Trigger words - "search feeds", "what's new in my feeds", "find articles about", "read from feeds", "daily briefing", "lumen".
metadata:
  author: allenchen
  version: '0.1'
  title: Lumen Feed Intelligence
  description_zh: 通过 Lumen CLI 搜索和检索 RSS 订阅文章，支持话题研究、每日简报、全文 Markdown 提取
---

# Lumen Feed Intelligence

Lumen is a local CLI that manages RSS feeds and exposes articles as structured JSON for agent consumption. All output is JSON envelopes. The GUI is for the human; this skill is for agents.

## When to Use

- User asks to search their feeds for a topic
- User wants a briefing on recent/unread articles
- User asks to read or analyze specific articles
- User mentions "lumen", "feeds", "subscriptions"

## Setup

The `lumen` binary must be in PATH or at the project's `target/debug/lumen` path. Test with:

```bash
lumen
```

This returns a self-describing JSON with all commands and the `compact_schema` explaining short field names.

## Compact Schema

Compact output uses short keys to save tokens:

| Key | Meaning |
|-----|---------|
| `id` | Article ID (use for all operations) |
| `t` | Title |
| `src` | Feed name |
| `tldr` | First sentence summary |
| `tags` | Fact-based tags: long, short, has_code, has_steps, has_images, has_links, structured, link_rich, has_references |
| `wc` | Word count |
| `r` | Read status (0/1) |
| `s` | Starred status (0/1) |

## Core Workflow

### 1. Scan — find articles (cheapest, ~50 tokens/article)

```bash
# Search by topic
lumen search "rust async" --compact --count 20

# Multi-topic with FTS5 boolean operators
lumen search "rust OR wasm OR async" --compact --since 7d

# Date range
lumen search "AI" --compact --after 2026-03-15 --before 2026-03-20

# Single day
lumen search "claude" --compact --on 2026-03-28

# All recent unread
lumen articles --compact --unread --count 50
```

### 2. Filter — use jq on compact output (zero cost, runs locally)

```bash
# Long unread articles with code
lumen articles --compact --count 200 | jq '[.result.articles[] | select(.r==0 and .wc > 1000 and (.tags | contains("has_code")))]'

# Extract IDs for batch operations
lumen search "topic" --compact | jq -r '[.result.articles[] | select(.wc > 500) | .id] | join(",")'
```

### 3. Retrieve — get full text as markdown files

```bash
# Single article
lumen fetch-full-text 401 --markdown
# Returns: {"id": 401, "path": "/tmp/lumen/401_title-slug.md", "wc": 3200}

# Batch
lumen fetch-full-text 401,402,403 --markdown

# Pipe from search
lumen search "rust" --compact | jq -r '[.result.articles[].id] | join(",")' | lumen fetch-full-text --markdown
```

After getting the file path, use `Read`, `Grep`, or `Bash` to analyze the markdown content directly. The full text never enters your context unless you choose to read it.

### 4. Act — mark articles

```bash
lumen star 401          # Star interesting article
lumen mark-read 401     # Mark as read
```

## One-shot Examples

**"What's new about AI in my feeds this week?"**
```bash
lumen search "AI OR LLM OR agent" --compact --since 7d --count 30
```
Then summarize the compact results for the user.

**"Find and read the best rust articles from March"**
```bash
lumen search "rust" --compact --after 2026-03-01 --before 2026-03-31 | jq '[.result.articles[] | select(.wc > 500)] | sort_by(-.wc)'
```
Pick top results, then `lumen fetch-full-text <ids> --markdown` and read the files.

**"Give me a daily briefing"**
```bash
lumen articles --compact --unread --count 100
```
Group by `src`, highlight high-wc and has_code articles, summarize for user.

## Common Mistakes

- **Don't read full HTML output** — always use `--markdown` flag, it writes clean markdown to a temp file
- **Don't fetch all articles then filter** — use `--compact` first, filter with jq, then fetch only what you need
- **Don't guess field names** — run `lumen` with no args to get `compact_schema`
- **Don't use `--since` for absolute dates** — use `--after`/`--before`/`--on` instead
