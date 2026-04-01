#!/bin/bash
PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$PROJECT_DIR"

echo "========================================="
echo "  Alignment Reflection Report"
echo "  $(date)"
echo "========================================="
echo ""

echo "--- Paul Graham Taste Alignment ---"
echo ""

# Check simplicity: count total lines of code
if command -v tokei &> /dev/null; then
    echo "[Simplicity] Code size:"
    tokei --sort lines 2>/dev/null || echo "  (tokei not available)"
else
    RUST_LOC=$(find . -name '*.rs' -not -path './target/*' | xargs wc -l 2>/dev/null | tail -1 || echo "0")
    TS_LOC=$(find . -name '*.ts' -o -name '*.tsx' | grep -v node_modules | xargs wc -l 2>/dev/null | tail -1 || echo "0")
    echo "  Rust: $RUST_LOC"
    echo "  TS/TSX: $TS_LOC"
fi
echo ""

# Check accessibility
echo "[Accessibility / NVDA Readiness]"
ARIA_COUNT=$(grep -r 'aria-' src/ 2>/dev/null | wc -l || echo "0")
ROLE_COUNT=$(grep -r 'role=' src/ 2>/dev/null | wc -l || echo "0")
SEMANTIC_COUNT=$(grep -rE '<(nav|main|article|section|header|footer|aside)' src/ 2>/dev/null | wc -l || echo "0")
echo "  ARIA attributes: $ARIA_COUNT"
echo "  Role attributes: $ROLE_COUNT"
echo "  Semantic HTML elements: $SEMANTIC_COUNT"
echo ""

# Check dependencies count (fewer = simpler)
echo "[Simplicity] Dependency count:"
if [ -f Cargo.lock ]; then
    CRATE_COUNT=$(grep -c '^\[\[package\]\]' Cargo.lock 2>/dev/null || echo "unknown")
    echo "  Rust crates: $CRATE_COUNT"
fi
if [ -f package-lock.json ] || [ -f pnpm-lock.yaml ]; then
    echo "  (Node deps: check package.json)"
fi
echo ""

# Check keyboard navigation support
echo "[Keyboard Navigation]"
KEYDOWN_COUNT=$(grep -r 'onKeyDown\|onkeydown\|keydown\|KeyboardEvent' src/ 2>/dev/null | wc -l || echo "0")
TABINDEX_COUNT=$(grep -r 'tabindex\|tabIndex' src/ 2>/dev/null | wc -l || echo "0")
echo "  Keyboard handlers: $KEYDOWN_COUNT"
echo "  Tabindex usage: $TABINDEX_COUNT"
echo ""

# Sprint alignment
echo "--- Sprint Progress ---"
if [ -f SPRINT_PLAN.md ]; then
    DONE=$(grep -c '\[x\]' SPRINT_PLAN.md || true)
    TODO=$(grep -c '\[ \]' SPRINT_PLAN.md || true)
    echo "  Completed: $DONE"
    echo "  Remaining: $TODO"
fi
echo ""

# Recent git activity
echo "--- Recent Commits ---"
git log --oneline -5 2>/dev/null || echo "  No commits yet"
echo ""

echo "========================================="
echo "  Reflection complete"
echo "========================================="
