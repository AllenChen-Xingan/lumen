#!/bin/bash
# NVDA Accessibility Structural Tests
# Checks the built HTML and source for NVDA compatibility patterns

PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$PROJECT_DIR"

echo "========================================="
echo "  NVDA Accessibility Structural Tests"
echo "  $(date)"
echo "========================================="
echo ""

PASS=0
FAIL=0
WARN=0

check() {
    local name="$1"
    local cmd="$2"
    echo -n "  $name... "
    if eval "$cmd" > /dev/null 2>&1; then
        echo "PASS"
        PASS=$((PASS + 1))
    else
        echo "FAIL"
        FAIL=$((FAIL + 1))
    fi
}

warn_check() {
    local name="$1"
    local cmd="$2"
    echo -n "  $name... "
    if eval "$cmd" > /dev/null 2>&1; then
        echo "PASS"
        PASS=$((PASS + 1))
    else
        echo "WARN"
        WARN=$((WARN + 1))
    fi
}

SRC="$PROJECT_DIR/src/App.tsx"
CSS="$PROJECT_DIR/src/styles.css"

echo "--- Critical: No NVDA Traps ---"
check 'No role="application" (traps NVDA users)' "! grep -q 'role=\"application\"' $SRC"
check 'No tabindex > 0 (breaks tab order)' "! grep -qE 'tabindex=\"[2-9]' $SRC"
check 'No Tab key override in handlers' "! grep -q 'e.key === \"Tab\"' $SRC"

echo ""
echo "--- ARIA Landmarks ---"
check 'Has <nav> landmark' "grep -q '<nav' $SRC"
check 'Has <main> landmark' "grep -q '<main' $SRC"
check 'Has aria-label on nav' "grep -q 'nav.*aria-label' $SRC"
check 'Has aria-label on main' "grep -A5 '<main' $SRC | grep -q aria-label"

echo ""
echo "--- Feed Pattern (W3C APG) ---"
check 'Uses role="feed" for article list' "grep -q 'role=\"feed\"' $SRC"
check 'Uses role="article" for items' "grep -q 'role=\"article\"' $SRC"
check 'Articles have aria-posinset' "grep -q 'aria-posinset' $SRC"
check 'Articles have aria-setsize' "grep -q 'aria-setsize' $SRC"
check 'Articles have tabindex="0"' "grep -q 'tabindex.*0' $SRC"

echo ""
echo "--- Heading Hierarchy ---"
check 'Has h1 (app title)' "grep -q '<h1' $SRC"
check 'Has h2 (pane titles)' "grep -q '<h2' $SRC"
check 'Has h3 (article title)' "grep -q '<h3' $SRC"

echo ""
echo "--- Live Regions ---"
check 'Has aria-live="polite"' "grep -q 'aria-live=\"polite\"' $SRC"
check 'Has aria-live="assertive"' "grep -q 'aria-live=\"assertive\"' $SRC"
check 'Has role="status"' "grep -q 'role=\"status\"' $SRC"

echo ""
echo "--- Keyboard Navigation ---"
check 'j/k navigation' "grep -q 'e.key === \"j\"' $SRC"
check 'Escape to go back' "grep -q 'e.key === \"Escape\"' $SRC"
check 'Enter to activate' "grep -q 'e.key === \"Enter\"' $SRC"
check 'n/p for unread' "grep -q 'e.key === \"n\"' $SRC"

echo ""
echo "--- Focus Management ---"
check 'Has skip link' "grep -q 'skip-link\|Skip to' $SRC"
check 'Focus visible styles exist' "grep -q 'focus-visible' $CSS"
check 'High contrast mode support' "grep -q 'forced-colors' $CSS"
check '.sr-only class exists' "grep -q 'sr-only' $CSS"

echo ""
echo "--- Form Accessibility ---"
check 'Input has label or aria-label' "grep -qE '(for=|aria-label=).*[Ff]eed.?URL|[Ss]earch' $SRC"
check 'Buttons have aria-label' "grep -c 'aria-label=' $SRC | awk '{exit (\$0 >= 5 ? 0 : 1)}'"

echo ""
echo "--- No Anti-Patterns ---"
check 'No role="listbox" (NVDA browse mode issue)' "! grep -q 'role=\"listbox\"' $SRC"
check 'No aria-activedescendant (unreliable)' "! grep -q 'aria-activedescendant' $SRC"
check 'No autofocus attribute' "! grep -q 'autofocus' $SRC"

echo ""
echo "========================================="
echo "  Results: $PASS pass, $FAIL fail, $WARN warn"
if [ $FAIL -eq 0 ]; then
    echo "  STATUS: ALL PASS"
else
    echo "  STATUS: $FAIL FAILURES - fix before release"
fi
echo "========================================="

exit $FAIL
