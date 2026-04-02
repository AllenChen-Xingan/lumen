Find relevant articles from my RSS feeds based on current context.

## Instructions

1. **Understand intent**: Read the conversation history, current project (CLAUDE.md, recent files), and any arguments the user passed after `/lumen`. Figure out what they're working on or curious about.

2. **Build queries**: Turn your understanding into 2-5 search queries using FTS5 syntax (OR for related terms). Examples:
   - Working on Rust CLI → `"rust OR CLI OR terminal"`
   - Discussing agent design → `"agent OR LLM OR tool-use"`
   - User said `/lumen supply chain security` → `"supply chain OR security OR dependency"`

3. **Search**: Run searches via Bash:
   ```bash
   lumen search "query" --compact --since 7d --count 30
   ```
   Use `--after`/`--before`/`--on` for specific dates. Use `jq` to filter: `select(.r==0 and .wc > 300)`.

4. **Pick top 3-5** articles based on: title/tldr relevance, word count > 300, diverse sources, useful tags (has_code, structured).

5. **Fetch markdown**:
   ```bash
   lumen fetch-full-text <id1>,<id2>,<id3> --markdown
   ```

6. **Verify**: Use Grep on the markdown files to confirm relevance before presenting.

7. **Report**: List each article with title, source, word count, tldr, and file path. Quote a key passage if one stands out.

$ARGUMENTS
