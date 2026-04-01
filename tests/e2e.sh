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
    if echo "$OUTPUT" | grep -q "$expect"; then
        echo "PASS"
        PASS=$((PASS + 1))
    else
        echo "FAIL (expected '$expect')"
        echo "  Got: $OUTPUT"
        FAIL=$((FAIL + 1))
    fi
}

echo "Running E2E tests..."
echo ""

# Test 1: Help
e2e_test "CLI help" "$CLI --help" "Minimalist RSS reader"

# Test 2: List empty
e2e_test "List empty feeds" "$CLI list" "No feeds"

# Test 3: Add a real feed (using a reliable test feed)
e2e_test "Add RSS feed" "$CLI add https://hnrss.org/newest?count=3" "Added feed"

# Test 4: List shows feed
e2e_test "List shows added feed" "$CLI list" "Hacker News"

# Test 5: Articles exist
e2e_test "Articles found" "$CLI articles" "\["

# Test 6: Fetch
e2e_test "Fetch works" "$CLI fetch" "Fetched"

# Test 7: Remove feed
e2e_test "Remove feed" "$CLI remove 1" "removed"

# Cleanup
rm -f "$TEST_DB"

echo ""
echo "E2E Results: $PASS passed, $FAIL failed"
[ $FAIL -eq 0 ] || exit 1
