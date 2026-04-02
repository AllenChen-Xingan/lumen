Give me a daily briefing of my RSS feeds.

## Instructions

1. **Fetch recent unread**:
   ```bash
   lumen articles --compact --unread --count 100
   ```

2. **Group by source** (`src` field) and count articles per source.

3. **Highlight interesting articles**: Pick articles that are:
   - Long form (wc > 1000) — deep reads
   - Has code (tags contains "has_code") — technical content
   - Structured (tags contains "structured" or "has_steps") — tutorials/guides

4. **Fetch top 3-5 for summaries**:
   ```bash
   lumen fetch-full-text <ids> --markdown
   ```
   Read first 50 lines of each markdown file to get the gist.

5. **Present as briefing**:
   - Total unread count and breakdown by source
   - "Worth reading" section: 3-5 articles with title, source, word count, and a 1-sentence why
   - "Quick scan" section: remaining notable titles grouped by source

$ARGUMENTS
