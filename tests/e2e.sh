#!/bin/bash
set -e

PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$PROJECT_DIR"

CLI="$PROJECT_DIR/target/debug/rss-cli"
if [ ! -f "$CLI" ] && [ ! -f "$CLI.exe" ]; then
    echo "Building CLI..."
    cargo build -p rss-cli 2>&1
fi

# Use .exe on Windows
if [ -f "$CLI.exe" ]; then
    CLI="$CLI.exe"
fi

TEST_DB="/tmp/rss-reader-test-$(date +%s).db"
export RSS_DB_PATH="$TEST_DB"

PASS=0
FAIL=0

e2e_test() {
    local name="$1"
    local cmd="$2"
    local expect="$3"
    echo -n "  E2E: $name... "
    OUTPUT=$(eval "$cmd" 2>&1) || true
    if echo "$OUTPUT" | grep -qE "$expect"; then
        echo "PASS"
        PASS=$((PASS + 1))
    else
        echo "FAIL (expected '$expect')"
        echo "  Got: $(echo "$OUTPUT" | head -c 200)"
        FAIL=$((FAIL + 1))
    fi
}

echo "Running E2E tests..."
echo ""

# Test 1: Help
e2e_test "CLI help" "$CLI --help" "Agent-native RSS reader"

# Test 2: No-args self-description (JSON envelope)
e2e_test "Self-describe" "$CLI" '"ok":true.*compact_schema'

# Test 3: List empty (JSON)
e2e_test "List empty feeds" "$CLI list" '"count":0'

# Test 4: Add a real feed
e2e_test "Add RSS feed" "$CLI add https://feeds.bbci.co.uk/news/technology/rss.xml" '"feed_id":'

# Test 5: List shows feed
e2e_test "List shows added feed" "$CLI list" "BBC"

# Test 6: Articles exist (JSON array)
e2e_test "Articles found" "$CLI articles --count 5" '"articles":\['

# Test 7: Compact output has short keys
e2e_test "Compact output" "$CLI articles --compact --count 1" '"src":.*"t":.*"wc":'

# Test 8: Search works
e2e_test "Search" "$CLI search BBC --compact --count 3" '"articles":\['

# Test 9: Search with --on
e2e_test "Search --on date" "$CLI search BBC --compact --on $(date +%Y-%m-%d)" '"ok":true'

# Test 10: Fetch updates
e2e_test "Fetch works" "$CLI fetch" '"feeds_fetched":'

# Test 11: Fetch-full-text single
FIRST_ID=$($CLI articles --compact --count 1 2>/dev/null | grep -o '"id":[0-9]*' | head -1 | grep -o '[0-9]*')
if [ -n "$FIRST_ID" ]; then
    e2e_test "Fetch-full-text single" "$CLI fetch-full-text $FIRST_ID" '"article_id":'

    # Test 12: Fetch-full-text --markdown
    e2e_test "Fetch-full-text markdown" "$CLI fetch-full-text $FIRST_ID --markdown" '"path":.*\.md.*"wc":'

    # Test 13: Batch fetch-full-text
    SECOND_ID=$($CLI articles --compact --count 2 2>/dev/null | grep -o '"id":[0-9]*' | tail -1 | grep -o '[0-9]*')
    if [ -n "$SECOND_ID" ] && [ "$SECOND_ID" != "$FIRST_ID" ]; then
        e2e_test "Batch fetch-full-text" "$CLI fetch-full-text $FIRST_ID,$SECOND_ID --markdown" '"items":\[.*"count":2'
    fi

    # Test 14: Pipe stdin
    e2e_test "Stdin pipe" "echo $FIRST_ID | $CLI fetch-full-text --markdown" '"path":.*\.md'
fi

# Test 15: Star
if [ -n "$FIRST_ID" ]; then
    e2e_test "Star article" "$CLI star $FIRST_ID" '"toggled":true'
fi

# Test 16: Folders
e2e_test "Folders list" "$CLI folders" '"folders":'

# Test 17: Feed-health
e2e_test "Feed health" "$CLI feed-health" '"feeds":'

# Test 18: Remove feed
e2e_test "Remove feed" "$CLI remove 1" "removed"

# Cleanup
rm -f "$TEST_DB"

echo ""
echo "E2E Results: $PASS passed, $FAIL failed"
[ $FAIL -eq 0 ] || exit 1
