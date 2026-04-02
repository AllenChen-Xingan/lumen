# Decision 002: CLI Friction Points from Real Agent Usage

**Date**: 2026-04-02
**Status**: Proposed
**Context**: First real agent-driven information retrieval session using the CLI. An agent (Claude Code) searched for "claude code" articles, triaged 20 results, read 3 full texts, and synthesized recommendations. This document records the friction points discovered.

## Session Summary

Task: "Find articles about Claude Code worth reading, based on current project context."

Flow used:
```
search "claude code" --compact --count 20   → scan 20 results
read 2927 / read 2952 / read 935            → browse 3 articles
fetch-full-text 2927 / 2952 / 935           → deep read 3 articles
```

Total CLI calls: 7 (would be 4 with batch support).

## Friction Point 1: Search Has No Relevance Ranking

**Problem**: `search "claude code"` returned 20 articles (of 139 total). `matched_in` is either `title` or `content`, but results are not sorted by relevance. Title matches and content matches are interleaved.

**Impact**: Agent must scan all 20 to find the most relevant, wasting tokens on content-matched articles that mention "claude" in passing.

**Proposed fix**: Sort results by relevance signal:
1. Title match > content match
2. Within same match type, sort by BM25 score (FTS5 already computes this)
3. Add `--sort relevance|date` flag (date is current default)

**Relates to**: ROADMAP phase 2 "Full-text search (SQLite FTS5) — BM25 relevance sorting"

## Friction Point 2: `read` Content Preview Too Short

**Problem**: `read 2927` returned `content_preview` of ~500 chars for a 434-word article. For longer articles, the truncation is worse. Agent can't judge whether to fetch-full-text without enough preview.

**Impact**: Agent must always fetch-full-text to make a relevance decision, defeating the 3-layer progressive disclosure model. The browse layer collapses into the deep layer.

**Proposed fix**: 
- Increase `content_preview` to 800-1000 chars
- Or better: implement the `outline` + `key_terms` browse layer from Decision 001. This gives the agent structural summary without full text.

**Relates to**: Decision 001 "Browse Layer is Lazy, Not Pre-computed"

## Friction Point 3: fetch-full-text HTML Too Noisy

**Problem**: Readability extraction includes navigation, footer, comment sections, subscribe forms. Example: antirez's blog post included `<article data-comment-id="160-">` wrapper with metadata. Joan Westenberg's paywalled article included the full site nav repeated 5 times.

**Impact**: Agent processing full text wastes tokens on non-content. When spawning a subagent to summarize, noise degrades quality.

**Proposed fix**:
- Add `--format markdown` output (strip HTML, clean nav/footer) — already in Decision 001
- Improve readability post-processing: strip `<nav>`, `<header>`, `<footer>`, comment sections, repeated link blocks
- Consider word count threshold: if extracted text < 100 words and contains "subscribe" / "sign in", flag as paywalled

**Relates to**: Decision 001 "Full Text Output as Markdown for Subagent Isolation"

## Friction Point 4: No Batch Operations

**Problem**: Reading 3 articles required 3 separate `read` commands + 3 `fetch-full-text` commands. Each incurs CLI startup + DB connection overhead.

**Impact**: 6 commands instead of 2. For an agent scanning 20 articles and reading 5, this is 25 commands instead of 3.

**Proposed fix**: 
- `read 2927,2952,935` — comma-separated IDs
- `fetch-full-text 2927,2952,935` — same pattern
- Return array of results in the JSON envelope

**Relates to**: ROADMAP phase 3 "Batch operations"

## Friction Point 5: Star Not Integrated into Search Flow

**Problem**: After search → read → decide "this is worth keeping", agent must run a separate `star <id>` command. No way to star during search or combine operations.

**Impact**: Minor — extra command. But in a triage workflow (search → scan → star interesting → batch read starred), the flow is clunky.

**Proposed fix**: Low priority. Consider `--star` flag on `read` command, or batch `star 2927,2952,935`.

## Friction Point 6: No Parallel/Batch Semantics

**Problem**: Every command operates on a single ID. Real agent workflows are batch-oriented: scan 20 → read 5 → star 2 → mark-read 18. Currently 28 CLI calls, should be ~6.

**Impact**: CLI startup + DB connection overhead per call. Agent context filled with repetitive tool invocations. Tauri frontend also suffers — `fetchAll` is serial per feed.

**Proposed design**: Three levels of batch support, inspired by shell glob/pipeline patterns.

### Level 1: Comma-separated IDs (like `cat a.md b.md c.md`)

All commands that accept a single `<id>` also accept `<id1>,<id2>,<id3>`:

```bash
rss read 2927,2952,935              # returns array of 3 articles
rss fetch-full-text 2927,2952,935   # parallel HTTP fetches, returns as each completes
rss star 2927,2952,935              # batch star
rss mark-read 2927,2952,935         # batch mark-read
```

Return format: `{"ok": true, "result": {"items": [...], "errors": [...]}}` — partial success is OK.

### Level 2: Predicate selectors (like `find . -name "*.log" -delete`)

Operate on sets defined by predicates, not enumerated IDs:

```bash
rss mark-read --feed 5                    # all articles in feed 5
rss mark-read --before 2026-03-01         # all articles before date
rss mark-read --folder 3                  # all articles in manual folder
rss fetch-full-text --unread --feed 5     # parallel fetch all unread in feed
rss star --search "claude code"           # star all search results
rss _annotate --unannotated --feed 5      # annotate only what's new in a feed
```

Safety: destructive predicates (mark-read --all) require `--confirm` or return a dry-run count first.

### Level 3: Pipe composition (like `find | xargs`)

Commands output IDs that can be piped into batch operations:

```bash
rss search "rust" --ids-only | rss batch-star
rss articles --feed 5 --unread --ids-only | rss batch-mark-read
rss articles --compact --count 200 | jq '[.result.articles[] | select(.s==0 and .r==0)] | .[].id' | rss batch-fetch-full-text
```

`--ids-only` outputs newline-separated IDs. `batch-*` commands read IDs from stdin.

### Parallel execution strategy

- `fetch-full-text` with multiple IDs: parallel HTTP (tokio::join or rayon), up to 4 concurrent
- `read` with multiple IDs: single SQL query `WHERE id IN (...)`
- `mark-read`/`star` with multiple IDs: single SQL `UPDATE ... WHERE id IN (...)`
- `fetch` with `--feed 5,9,12`: parallel feed fetching (already exists for fetchAll)

**Relates to**: ROADMAP phase 3 "Batch operations"

## Priority Order for Next Session

Based on impact to agent efficiency:

1. **Comma-separated IDs on all commands** (Friction 6 Level 1) — lowest effort, highest multiplier. Single SQL `WHERE IN`, parallel HTTP for fetch-full-text. Unblocks everything else.
2. **Search relevance sorting** (Friction 1) — FTS5 already computes BM25, just wire it up
3. **Browse layer implementation** (Friction 2) — outline + key_terms makes the 3-layer model actually work
4. **Predicate selectors** (Friction 6 Level 2) — `mark-read --feed 5`, `star --search "query"`
5. **Markdown full-text output** (Friction 3) — cleaner subagent input
6. **Pipe composition** (Friction 6 Level 3) — `--ids-only` + `batch-*` stdin readers
7. **Star integration** (Friction 5) — nice to have

## Evidence from This Session

- Agent made correct triage decisions on 3/3 articles selected for deep read (all were genuinely relevant)
- The 3-layer model conceptually works, but browse layer doesn't exist yet — agent jumped from scan directly to deep
- Total tokens consumed: ~20K for search+read+fetch of 3 articles. With proper browse layer, could be ~8K (outline+key_terms instead of full HTML)
- Paywalled content detection would have saved 1 fetch-full-text call (Westenberg article)
