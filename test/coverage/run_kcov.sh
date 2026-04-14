#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Optional: kcov-based coverage (requires kcov installed)
#
# Install: brew install kcov
#
# Usage:
#   bash test/coverage/run_kcov.sh                          # all tests
#   bash test/coverage/run_kcov.sh test/unit/test_common.sh  # single test
#
# Output:
#   .reports/coverage/kcov/      kcov HTML + Cobertura XML
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
KCOV_OUT="$ROOT_DIR/.reports/coverage/kcov"

GREEN='\033[0;32m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m'

if ! command -v kcov &>/dev/null; then
    echo -e "${RED}kcov is not installed.${NC}"
    echo ""
    echo "Install options:"
    echo "  macOS:  brew install kcov"
    echo "  Linux:  apt install kcov  (or build from source)"
    echo "  Source: https://github.com/SimonKagstrom/kcov"
    echo ""
    echo "Alternatively, use the zero-dependency coverage runner:"
    echo "  bash test/coverage/run_coverage.sh"
    exit 1
fi

# Determine test files
if [[ $# -gt 0 ]]; then
    TEST_FILES=("$@")
else
    TEST_FILES=()
    for f in "$ROOT_DIR"/test/unit/test_*.sh; do
        [[ -f "$f" ]] && TEST_FILES+=("$f")
    done
fi

echo ""
echo -e "${BLUE}╔══════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║     workhive kcov Coverage Runner             ║${NC}"
echo -e "${BLUE}╚══════════════════════════════════════════════╝${NC}"
echo ""

rm -rf "$KCOV_OUT"
mkdir -p "$KCOV_OUT"

INCLUDE_PATTERN="$ROOT_DIR/lib/,$ROOT_DIR/bin/"

for test_file in "${TEST_FILES[@]}"; do
    test_name=$(basename "$test_file" .sh)
    echo -e "  ${BLUE}Running: $test_name${NC}"

    kcov --include-pattern="$INCLUDE_PATTERN" \
         --exclude-pattern=test/,/tmp/,.git/ \
         --bash-dont-parse-binary-dir \
         "$KCOV_OUT/$test_name" \
         bash "$test_file" >/dev/null 2>&1 || true
done

# Merge results
if [[ $(ls -d "$KCOV_OUT"/test_* 2>/dev/null | wc -l | tr -d ' ') -gt 1 ]]; then
    echo ""
    echo -e "  ${BLUE}Merging results...${NC}"
    kcov --merge "$KCOV_OUT/merged" "$KCOV_OUT"/test_* 2>/dev/null || true
fi

echo ""
echo -e "${GREEN}Coverage report: $KCOV_OUT/${NC}"
echo ""

# Try to find and display summary
if [[ -f "$KCOV_OUT/merged/index.html" ]]; then
    echo -e "  Open: ${BLUE}open $KCOV_OUT/merged/index.html${NC}"
elif [[ -d "$KCOV_OUT" ]]; then
    first_report=$(ls -d "$KCOV_OUT"/test_*/index.html 2>/dev/null | head -1)
    if [[ -n "$first_report" ]]; then
        echo -e "  Open: ${BLUE}open $(dirname "$first_report")/index.html${NC}"
    fi
fi
echo ""
