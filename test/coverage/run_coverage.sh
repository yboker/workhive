#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Bash Code Coverage Runner
# Zero-dependency coverage via bash xtrace (PS4 + BASH_XTRACEFD)
#
# Usage:
#   bash test/coverage/run_coverage.sh                     # run all tests with coverage
#   bash test/coverage/run_coverage.sh test/unit/test_common.sh  # single test
#   COVERAGE_FORMAT=html bash test/coverage/run_coverage.sh      # generate HTML report
#
# Output:
#   .reports/coverage/            Coverage data and reports
#   .reports/coverage/summary.txt Coverage summary table
#   .reports/coverage/coverage.json  Machine-readable coverage data
#   .reports/coverage/html/       HTML report (when COVERAGE_FORMAT=html)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"

COVERAGE_DIR="$ROOT_DIR/.reports/coverage"
TRACE_DIR="$COVERAGE_DIR/traces"
COVERAGE_FORMAT="${COVERAGE_FORMAT:-text}"

mkdir -p "$TRACE_DIR" "$COVERAGE_DIR"

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# ──────────────────────────────────────────────────
# Determine which test files to run
# ──────────────────────────────────────────────────
if [[ $# -gt 0 ]]; then
    TEST_FILES=("$@")
else
    TEST_FILES=()
    for f in "$ROOT_DIR"/test/unit/test_*.sh; do
        [[ -f "$f" ]] && TEST_FILES+=("$f")
    done
fi

if [[ ${#TEST_FILES[@]} -eq 0 ]]; then
    echo -e "${RED}No test files found.${NC}"
    exit 1
fi

echo ""
echo -e "${BLUE}╔══════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║     workhive Coverage Runner                 ║${NC}"
echo -e "${BLUE}╚══════════════════════════════════════════════╝${NC}"
echo ""

# ──────────────────────────────────────────────────
# Collect source files to measure
# ──────────────────────────────────────────────────
SOURCE_FILES=()
while IFS= read -r f; do
    SOURCE_FILES+=("$f")
done < <(find "$ROOT_DIR/lib" -name '*.sh' -type f | sort)
SOURCE_FILES+=("$ROOT_DIR/bin/workhive")

echo -e "  Source files:  ${#SOURCE_FILES[@]}"
echo -e "  Test files:    ${#TEST_FILES[@]}"
echo ""

# ──────────────────────────────────────────────────
# Run tests with xtrace and capture executed lines
# ──────────────────────────────────────────────────

rm -rf "${TRACE_DIR:?}"/*

TOTAL_SUITES=0
PASSED_SUITES=0

for test_file in "${TEST_FILES[@]}"; do
    test_name=$(basename "$test_file")
    trace_file="$TRACE_DIR/${test_name}.trace"
    TOTAL_SUITES=$((TOTAL_SUITES + 1))

    echo -e "  ${BLUE}Running: $test_name${NC}"

    # PS4 outputs "TRACE:<source_file>:<line_number>: " for each executed line
    # BASH_XTRACEFD redirects xtrace to fd 3, keeping stdout/stderr clean
    env PS4='+TRACE:${BASH_SOURCE[0]:-unknown}:${LINENO}: ' \
        BASH_XTRACEFD=3 \
        bash -x "$test_file" 3>"$trace_file" >/dev/null 2>&1
    rc=$?

    if [[ $rc -eq 0 ]]; then
        PASSED_SUITES=$((PASSED_SUITES + 1))
        echo -e "    ${GREEN}✓ passed${NC}"
    else
        echo -e "    ${RED}✗ failed (rc=$rc)${NC}"
    fi
done

echo ""
echo -e "  Tests: ${PASSED_SUITES}/${TOTAL_SUITES} passed"
echo ""

# ──────────────────────────────────────────────────
# Parse traces to build coverage data
# ──────────────────────────────────────────────────

echo -e "${BLUE}Analyzing coverage...${NC}"

# Merge all trace files: extract unique source:line pairs
declare -A EXECUTED_LINES
for trace_file in "$TRACE_DIR"/*.trace; do
    [[ -f "$trace_file" ]] || continue
    while IFS= read -r line; do
        # Extract "+TRACE:/path/to/file.sh:42: ..." format
        if [[ "$line" =~ ^\+TRACE:([^:]+):([0-9]+): ]]; then
            src="${BASH_REMATCH[1]}"
            lineno="${BASH_REMATCH[2]}"
            # Normalize path
            if [[ "$src" == "$ROOT_DIR/"* ]]; then
                rel="${src#$ROOT_DIR/}"
                EXECUTED_LINES["$rel:$lineno"]=1
            elif [[ "$src" == /* ]]; then
                continue
            else
                EXECUTED_LINES["$src:$lineno"]=1
            fi
        fi
    done < "$trace_file"
done

# ──────────────────────────────────────────────────
# Calculate per-file coverage
# ──────────────────────────────────────────────────

SUMMARY_FILE="$COVERAGE_DIR/summary.txt"
JSON_FILE="$COVERAGE_DIR/coverage.json"

{
    printf '%-55s %8s %8s %8s\n' "File" "Total" "Hit" "Coverage"
    printf '%-55s %8s %8s %8s\n' "----" "-----" "---" "--------"
} > "$SUMMARY_FILE"

TOTAL_EXECUTABLE=0
TOTAL_HIT=0

json_entries=()

for src_file in "${SOURCE_FILES[@]}"; do
    [[ -f "$src_file" ]] || continue
    rel_path="${src_file#$ROOT_DIR/}"

    # Count executable lines (non-blank, non-comment, non-shebang)
    executable=0
    hit=0
    lineno=0

    while IFS= read -r code_line; do
        lineno=$((lineno + 1))
        trimmed="${code_line#"${code_line%%[![:space:]]*}"}"

        # Skip non-executable lines
        [[ -z "$trimmed" ]] && continue
        [[ "$trimmed" == '#'* ]] && continue
        [[ "$trimmed" == '#!/'* ]] && continue
        # Skip pure closing braces/keywords
        case "$trimmed" in
            '}') continue ;;
            'fi') continue ;;
            'done') continue ;;
            'esac') continue ;;
            'else') continue ;;
            'then') continue ;;
            'do') continue ;;
            ';;') continue ;;
            'EOF') continue ;;
        esac

        executable=$((executable + 1))

        if [[ -n "${EXECUTED_LINES[$rel_path:$lineno]:-}" ]]; then
            hit=$((hit + 1))
        fi
    done < "$src_file"

    TOTAL_EXECUTABLE=$((TOTAL_EXECUTABLE + executable))
    TOTAL_HIT=$((TOTAL_HIT + hit))

    if [[ $executable -gt 0 ]]; then
        pct=$((hit * 100 / executable))
    else
        pct=0
    fi

    printf '%-55s %8d %8d %7d%%\n' "$rel_path" "$executable" "$hit" "$pct" >> "$SUMMARY_FILE"
    json_entries+=("{\"file\":\"$rel_path\",\"executable\":$executable,\"hit\":$hit,\"coverage\":$pct}")
done

if [[ $TOTAL_EXECUTABLE -gt 0 ]]; then
    TOTAL_PCT=$((TOTAL_HIT * 100 / TOTAL_EXECUTABLE))
else
    TOTAL_PCT=0
fi

{
    printf '%-55s %8s %8s %8s\n' "----" "-----" "---" "--------"
    printf '%-55s %8d %8d %7d%%\n' "TOTAL" "$TOTAL_EXECUTABLE" "$TOTAL_HIT" "$TOTAL_PCT"
} >> "$SUMMARY_FILE"

# Generate JSON
{
    echo "{"
    echo "  \"timestamp\": \"$(date -u +%Y-%m-%dT%H:%M:%SZ)\","
    echo "  \"total_executable\": $TOTAL_EXECUTABLE,"
    echo "  \"total_hit\": $TOTAL_HIT,"
    echo "  \"total_coverage\": $TOTAL_PCT,"
    echo "  \"files\": ["
    local_first=true
    for entry in "${json_entries[@]}"; do
        if [[ "$local_first" == true ]]; then
            local_first=false
        else
            echo ","
        fi
        printf "    %s" "$entry"
    done
    echo ""
    echo "  ]"
    echo "}"
} > "$JSON_FILE"

# ──────────────────────────────────────────────────
# Generate HTML report (optional)
# ──────────────────────────────────────────────────

if [[ "$COVERAGE_FORMAT" == "html" ]]; then
    HTML_DIR="$COVERAGE_DIR/html"
    mkdir -p "$HTML_DIR"

    cat > "$HTML_DIR/index.html" << 'HTMLEOF'
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<title>workhive Coverage Report</title>
<style>
body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; margin: 2rem; background: #1a1b26; color: #c0caf5; }
h1 { color: #7aa2f7; }
table { border-collapse: collapse; width: 100%; margin-top: 1rem; }
th, td { text-align: left; padding: 0.5rem 1rem; border-bottom: 1px solid #3b4261; }
th { background: #24283b; color: #7aa2f7; }
tr:hover { background: #24283b; }
.pct-high { color: #9ece6a; }
.pct-mid { color: #e0af68; }
.pct-low { color: #f7768e; }
.total { font-weight: bold; background: #24283b; }
.bar { display: inline-block; height: 12px; border-radius: 3px; }
.bar-bg { background: #3b4261; width: 100px; display: inline-block; border-radius: 3px; }
</style>
</head>
<body>
<h1>workhive Coverage Report</h1>
<p id="timestamp"></p>
<table>
<thead><tr><th>File</th><th>Executable</th><th>Hit</th><th>Coverage</th><th></th></tr></thead>
<tbody id="tbody"></tbody>
</table>
<script>
HTMLEOF

    echo "const data = $(cat "$JSON_FILE");" >> "$HTML_DIR/index.html"

    cat >> "$HTML_DIR/index.html" << 'HTMLEOF2'
document.getElementById('timestamp').textContent = 'Generated: ' + data.timestamp;
const tbody = document.getElementById('tbody');
data.files.forEach(f => {
    const cls = f.coverage >= 80 ? 'pct-high' : f.coverage >= 50 ? 'pct-mid' : 'pct-low';
    const row = `<tr><td>${f.file}</td><td>${f.executable}</td><td>${f.hit}</td>
    <td class="${cls}">${f.coverage}%</td>
    <td><span class="bar-bg"><span class="bar" style="width:${f.coverage}px;background:${f.coverage >= 80 ? '#9ece6a' : f.coverage >= 50 ? '#e0af68' : '#f7768e'}"></span></span></td></tr>`;
    tbody.innerHTML += row;
});
tbody.innerHTML += `<tr class="total"><td>TOTAL</td><td>${data.total_executable}</td><td>${data.total_hit}</td>
<td class="${data.total_coverage >= 80 ? 'pct-high' : data.total_coverage >= 50 ? 'pct-mid' : 'pct-low'}">${data.total_coverage}%</td>
<td><span class="bar-bg"><span class="bar" style="width:${data.total_coverage}px;background:${data.total_coverage >= 80 ? '#9ece6a' : data.total_coverage >= 50 ? '#e0af68' : '#f7768e'}"></span></span></td></tr>`;
</script>
</body>
</html>
HTMLEOF2

    echo -e "${GREEN}HTML report: $HTML_DIR/index.html${NC}"
fi

# ──────────────────────────────────────────────────
# Print summary
# ──────────────────────────────────────────────────

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Coverage Summary${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""

# Show top uncovered files
echo -e "  ${YELLOW}Lowest coverage files:${NC}"
# macOS head doesn't support -n -2, use awk to skip header and footer
awk 'NR>2 && !/^----/ && !/^TOTAL/' "$SUMMARY_FILE" | sort -t'%' -k1 -n | head -10 | while IFS= read -r line; do
    pct=$(echo "$line" | grep -oE '[0-9]+%$' | tr -d '%')
    if [[ "${pct:-0}" -lt 50 ]]; then
        echo -e "    ${RED}$line${NC}"
    elif [[ "${pct:-0}" -lt 80 ]]; then
        echo -e "    ${YELLOW}$line${NC}"
    else
        echo -e "    ${GREEN}$line${NC}"
    fi
done

echo ""

if [[ $TOTAL_PCT -ge 80 ]]; then
    color="$GREEN"
elif [[ $TOTAL_PCT -ge 50 ]]; then
    color="$YELLOW"
else
    color="$RED"
fi

echo -e "  Total: ${color}${TOTAL_HIT}/${TOTAL_EXECUTABLE} lines covered (${TOTAL_PCT}%)${NC}"
echo ""
echo -e "  Full report: $SUMMARY_FILE"
echo -e "  JSON data:   $JSON_FILE"
echo ""
