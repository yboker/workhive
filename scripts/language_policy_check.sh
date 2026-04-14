#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

tmp_all="$(mktemp)"
tmp_violation="$(mktemp)"
trap 'rm -f "$tmp_all" "$tmp_violation"' EXIT

rg -n --pcre2 "[\\x{3400}-\\x{4DBF}\\x{4E00}-\\x{9FFF}]" -S . > "$tmp_all" || true

is_allowed_file() {
    local path="$1"

    case "$path" in
        ./README.zh-CN.md|./help/reference/*.zh-CN.md|./configs/*.zh-CN.md)
            return 0
            ;;
        ./lib/app/shared/help_index.sh|./lib/app/commands/tools_command.sh)
            return 0
            ;;
        ./test/*|./tests/*)
            return 0
            ;;
    esac

    return 1
}

while IFS= read -r line; do
    [[ -n "$line" ]] || continue
    path="${line%%:*}"

    if is_allowed_file "$path"; then
        continue
    fi

    if [[ "$path" == "./lib/utils/i18n.sh" ]]; then
        if [[ "$line" =~ _I18N_zh_[A-Za-z0-9_]+= ]]; then
            continue
        fi
    fi

    printf '%s\n' "$line" >> "$tmp_violation"
done < "$tmp_all"

if [[ -s "$tmp_violation" ]]; then
    echo "Language policy violations found:"
    cat "$tmp_violation"
    exit 1
fi

echo "Language policy check passed."
