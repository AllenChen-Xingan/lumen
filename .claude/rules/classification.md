---
description: No AI classification — fact-based annotation only, implicit feedback only
globs: ["crates/rss-ner/**", "crates/rss-cli/**"]
---

# Classification & Feedback Rules

1. **Fact-based annotation only.** `rss-ner` does deterministic text feature detection: word count, has_images, heading count, blockquote count, external link count. No embeddings, no LLM calls, no cosine similarity.

2. **Smart views are interaction-mode views.** Each must map to a user action (unread → triage, long → allocate time, recent → freshness check), not classify article type. Text features like has_code/has_steps are search filters, not views.

3. **No "AI guesses user intent" features.** The system cannot know what an article "means" to the user. That judgment belongs to the user or their agent.

4. **Implicit feedback only.** `is_read`, `is_starred`, `full_content IS NOT NULL` are the signals. Never add explicit feedback forms, tag correction UI, rating commands, or feedback tables.

5. **Behavior data is for ranking, not classification.** Per-feed engagement rates (read_rate, star_rate) can improve search result ordering, but never auto-assign categories.
