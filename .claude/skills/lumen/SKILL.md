---
name: lumen
description: "Find relevant articles from RSS feeds based on current context. Use when: user says /lumen, asks about feeds, wants inspiration, or says 'find articles about'. Scans conversation context to understand intent, searches feeds, retrieves markdown for deep reading."
metadata:
  author: allenchen
  version: '0.2'
  title: Lumen Feed Intelligence
  description_zh: 基于当前上下文自动从 RSS 订阅中发现相关文章
---

# Lumen — Context-Aware Feed Intelligence

## What This Does

When invoked, you act as an information retrieval agent:

1. **Read the room** — scan the conversation, current files, CLAUDE.md, and user's message to understand what they're working on or curious about
2. **Extract search intent** — turn that understanding into concrete search queries
3. **Search feeds** — call `lumen` CLI to find matching articles
4. **Triage** — filter compact results by relevance, quality (wc, tags), and recency
5. **Retrieve** — fetch top picks as markdown files
6. **Report** — summarize what you found, with file paths for deep reading

## Step 1: Understand Intent

Before touching lumen, figure out what the user actually wants. Sources of intent:

- **Explicit**: user said "find articles about X" → search X
- **Conversational**: user has been discussing topic Y for the last few messages → search Y
- **Project**: CLAUDE.md or recent files reveal the project domain → search domain keywords
- **Exploratory**: user said "what's interesting" or "inspire me" → scan unread, surface high-wc diverse articles

Combine these into 2-5 search queries. Use OR for related terms:
- Working on Rust CLI → `"rust OR CLI OR terminal"`
- Discussing agent architecture → `"agent OR LLM OR tool-use OR orchestration"`
- Just wants inspiration → skip search, use `lumen articles --compact --unread --count 100`

If the user provided arguments after `/lumen`, treat those as the primary intent. Example: `/lumen rust async patterns` → search "rust OR async" directly.

## Step 2: Search and Filter

Use a background Agent to run the search so it doesn't pollute main context:

```bash
# Topic search
lumen search "query1 OR query2" --compact --since 7d --count 30

# Or broader recent scan  
lumen articles --compact --unread --count 100
```

Filter with jq before fetching full text:
```bash
# Keep only substantial unread articles
| jq '[.result.articles[] | select(.r==0 and .wc > 300)] | sort_by(-.wc) | .[0:10]'
```

Pick top 3-5 articles based on:
- Title/tldr relevance to the intent
- `wc > 300` (substantial content)
- `tags` containing useful signals (structured for well-organized content, link_rich for well-sourced content)
- Diverse `src` (don't return 5 articles from one feed)

## Step 3: Retrieve as Markdown

```bash
lumen fetch-full-text <id1>,<id2>,<id3> --markdown
```

Returns file paths. Use `Read` or `Grep` on the markdown files to verify relevance before presenting to user.

## Step 4: Report to User

Present results as a concise list:

```
Found 4 articles related to [intent summary]:

1. **Title** (source, 2300 words)
   tldr summary
   → /tmp/lumen/401_title-slug.md

2. **Title** (source, 1500 words, has_code)
   tldr summary  
   → /tmp/lumen/402_title-slug.md
```

If a file is particularly relevant, read a key section and quote it.

## Compact Schema Quick Reference

| Key | Meaning |
|-----|---------|
| `id` | Article ID |
| `t` | Title |
| `src` | Feed name |
| `tldr` | First sentence |
| `tags` | long, short, has_code, has_steps, has_images, structured, link_rich |
| `wc` | Word count |
| `r` | Read (0/1) |
| `s` | Starred (0/1) |

## Search Flags

| Flag | Use |
|------|-----|
| `--compact` | Always use for scan phase |
| `--since 7d` | Relative time: 24h, 7d, 30d |
| `--after 2026-03-15` | Absolute start date |
| `--before 2026-03-20` | Absolute end date |
| `--on 2026-03-15` | Single day |
| `--feed 5` | Limit to one feed |
| `--count 50` | Max results |

## Common Mistakes

- **Don't skip intent analysis** — blindly searching the user's literal words misses what they actually need
- **Don't fetch full text before filtering** — compact scan first, always
- **Don't dump 20 articles** — pick 3-5 best, user can ask for more
- **Don't read markdown into main context** — use Grep to check relevance, only Read specific sections worth quoting
