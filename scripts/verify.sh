#!/bin/bash
set -e

PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$PROJECT_DIR"

echo "========================================="
echo "  RSS Reader Verification Report"
echo "  $(date)"
echo "========================================="
echo ""

PASS=0
FAIL=0
TOTAL=0

check() {
    local name="$1"
    local cmd="$2"
    TOTAL=$((TOTAL + 1))
    echo -n "[$TOTAL] $name... "
    if eval "$cmd" > /tmp/verify_output.txt 2>&1; then
        echo "PASS"
        PASS=$((PASS + 1))
    else
        echo "FAIL"
        cat /tmp/verify_output.txt | tail -5
        FAIL=$((FAIL + 1))
    fi
}

echo "--- Build Checks ---"
check "Rust workspace compiles" "cargo build 2>&1"
check "Rust tests pass" "cargo test 2>&1"

echo ""
echo "--- CLI Smoke Tests ---"
CLI_BIN="$PROJECT_DIR/target/debug/rss-cli"
if [ -f "$CLI_BIN" ] || [ -f "$CLI_BIN.exe" ]; then
    check "CLI --help works" "$CLI_BIN --help"
    check "CLI list works (empty)" "$CLI_BIN list"
else
    echo "CLI binary not found, skipping CLI tests"
    TOTAL=$((TOTAL + 2))
    FAIL=$((FAIL + 2))
fi

echo ""
echo "--- E2E Tests ---"
if [ -f "$PROJECT_DIR/tests/e2e.sh" ]; then
    check "E2E tests pass" "bash $PROJECT_DIR/tests/e2e.sh"
else
    echo "No E2E tests found yet"
fi

echo ""
echo "--- Frontend Checks ---"
if [ -f "$PROJECT_DIR/src/package.json" ] || [ -f "$PROJECT_DIR/package.json" ]; then
    check "Frontend builds" "cd $PROJECT_DIR && npm run build 2>&1 || pnpm build 2>&1"
else
    echo "No frontend found yet, skipping"
fi

echo ""
echo "--- Sprint Alignment ---"
if [ -f "$PROJECT_DIR/SPRINT_PLAN.md" ]; then
    DONE=$(grep -c '\[x\]' "$PROJECT_DIR/SPRINT_PLAN.md" || true)
    TODO=$(grep -c '\[ \]' "$PROJECT_DIR/SPRINT_PLAN.md" || true)
    TOTAL_ITEMS=$((DONE + TODO))
    if [ $TOTAL_ITEMS -gt 0 ]; then
        PCT=$((DONE * 100 / TOTAL_ITEMS))
    else
        PCT=0
    fi
    echo "Sprint progress: $DONE/$TOTAL_ITEMS ($PCT%)"
fi

echo ""
echo "========================================="
echo "  Results: $PASS/$TOTAL passed"
if [ $FAIL -eq 0 ]; then
    echo "  STATUS: ALL PASS"
else
    echo "  STATUS: $FAIL FAILURES"
fi
echo "========================================="

exit $FAIL
