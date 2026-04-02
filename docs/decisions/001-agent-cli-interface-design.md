# Decision 001: Agent CLI Interface Design

**Date**: 2026-04-02
**Status**: Accepted
**Context**: RSS Feed Manager is an agent-native information system. This document records the design decisions for how agents consume data from the system.

## Core Principle

**The system provides first-order facts. Agents provide second-order judgment.**

A first-order fact is something the machine can determine with certainty from text alone (word count, heading count, has code blocks). A second-order judgment requires intent ("this article is relevant to my task"). The system never crosses this boundary.

## Decision 1: Three-Layer Data Access (Coarse to Fine)

Agents access data through a progressive disclosure model. Each layer costs more tokens but provides more detail.

| Layer | Command | Per-article tokens | What agent gets |
|-------|---------|-------------------|-----------------|
| **Scan** | `rss articles --compact` | ~50 | id, title, tldr, tags, wc, r, s, source |
| **Browse** | `rss browse <id>` | ~100-150 | outline (heading texts), key_terms (top frequency words), structure fingerprint |
| **Deep** | `rss fetch-full-text <id>` | ~3000-5000 | Full article as markdown, processed in isolated subagent context |

Agent decides when to go deeper. The system never pushes data the agent didn't ask for.

## Decision 2: Behavior Data as Raw Signals, Not Scores

Behavior signals exposed per article:
- `r` (is_read): 0/1
- `s` (is_starred): 0/1
- `full_content IS NOT NULL`: implicit (article has been deep-read)

Behavior signals exposed per feed:
- `read_rate`: fraction of articles read in last 30 days
- `star_rate`: fraction of articles starred in last 30 days

**Not done**: No engagement_score, no importance_score, no recommendation ranking. These are second-order judgments. Different agents with different tasks interpret the same `read_rate: 0.02` differently ("skip this feed" vs "surface the 2% this user chose to read").

## Decision 3: Browse Layer is Lazy, Not Pre-computed

`outline` and `key_terms` are extracted on demand, not during refresh.

Reason: Most articles only have RSS summary at refresh time. Extracting outline from a 2-sentence summary is garbage-in-garbage-out. Extraction requires full_content or sufficiently long content (>500 words).

Flow:
```
rss browse <id>
  → full_content exists? → extract from it, cache in DB
  → content > 500 words? → extract from it, mark as partial
  → otherwise → fetch-full-text first, then extract
```

Once extracted, results are cached. Same article doesn't get processed twice.

## Decision 4: JSON Envelope + jq = Agent's Programming Interface

The system does NOT need to anticipate every query pattern. It provides:
- Consistent JSON envelope on every command
- Every field is a deterministic fact
- `next_actions` for discoverability (HATEOAS pattern)

Agents compose queries themselves:
```bash
# Find starred articles with code from high-engagement feeds
rss articles --compact --count 200 | jq '[.result.articles[] | select(.s==1 and (.tags | contains("has_code")))]'
```

This is why CLI beats MCP for this use case: no schema pre-loading cost (~35x token savings), agents already know how to use shell + jq from training data.

## Decision 5: Compact Output Field Design

Minimal keys for token efficiency:

```json
{"id":401, "t":"Title", "tldr":"First sentence...", "tags":"long,structured,has_code", "wc":3200, "r":1, "s":1, "src":"Without Boats"}
```

Every field serves the agent's decision funnel:
- `tags` → exclude (structural filtering)
- `tldr` → initial relevance check
- `wc` → cost estimation (how many tokens to read this)
- `r`/`s` → behavioral signal (user engagement)
- `src` → trust/authority signal (feed reputation)

## Decision 6: Full Text Output as Markdown for Subagent Isolation

When agent requests deep access, output is markdown (not HTML) written to a temp file:
```bash
rss fetch-full-text <id> --format markdown --output /tmp/article_401.md
```

Agent spawns a subagent to process the file. Subagent's full context is isolated and destroyed after returning a structured summary (~150 tokens). The 3000-5000 token article never enters the main agent's context window.

## Evidence Base

- **CLI vs MCP token cost**: 35x reduction measured in real benchmarks (jannikreinhard.com, 2026-02)
- **TOON vs JSON**: 40-60% token savings for scan data, but lower parse reliability (jduncan.io, 2025-11)
- **Claude Code subagent protocol**: Result capped at 100K chars, only final text returns to parent, all intermediate tool calls stay isolated (source: Claude Code source analysis)
- **JoelClaw CLI design**: JSON envelope + next_actions pattern independently converged to same design as this project (joelclaw.com, 2026-02)
- **agent-feeds library**: Demonstrated Markdown-native output for LLM consumption; validated the "URLs in, structured text out" pattern (rjocoleman/agent-feeds, 2026-03)

## What This Means for Implementation

Current state and gaps:

| Capability | Status | Gap |
|-----------|--------|-----|
| Scan layer (`--compact`) | Done | Shorten field names (t, wc, r, s, src) |
| Tags (fact-based) | Done | 9 tags including structured, link_rich, has_references |
| Behavior signals (r/s) | Done in DB | Not yet in compact output |
| Feed-level read_rate/star_rate | Done in DB | Not yet in `rss list` output |
| Browse layer (outline + key_terms) | Not started | Need extract_outline(), extract_key_terms() in rss-ner |
| Markdown full-text output | Not started | Need `--format markdown` flag on fetch-full-text |
| Browse caching in DB | Not started | Need outline, key_terms columns |
