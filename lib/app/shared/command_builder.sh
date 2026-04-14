#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Shared Pane Command List Builder (App Layer)
#
# Responsibility: Build pane command selection list, preferring tools registry,
#                 falling back to hardcoded detection for backward compatibility
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent duplicate loading
[[ -n "${_COMMAND_BUILDER_SH_LOADED:-}" ]] && return 0
readonly _COMMAND_BUILDER_SH_LOADED=1

# Dependencies
source "${LIB_DIR}/utils/i18n.sh"
source "${LIB_DIR}/infra/tools/registry.sh"

# ──────────────────────────────────────────────────
# Build available pane command labels and values
# Populates:
#   _AVAILABLE_CMD_LABELS   - display labels array
#   _AVAILABLE_CMD_VALUES   - command values array
#   _AVAILABLE_CMD_DEFAULT_IDX - 0-based index of the default tool
#   _AVAILABLE_CMD_EMPTY_IDX   - 0-based index of the empty shell option
# ──────────────────────────────────────────────────
_build_pane_command_list() {
    _AVAILABLE_CMD_LABELS=()
    _AVAILABLE_CMD_VALUES=()
    _AVAILABLE_CMD_DEFAULT_IDX=0
    _AVAILABLE_CMD_DEFAULT_REF=""

    local used_registry=false

    # Prefer reading from tools registry
    if check_cmd yq; then
        _AVAILABLE_CMD_DEFAULT_REF=$(tools_registry_get_default_ref 2>/dev/null || echo "")
        local registry_out
        registry_out=$(tools_registry_list 2>/dev/null || echo "")
        if [[ -n "$registry_out" ]]; then
            used_registry=true
            local i=0
            # shellcheck disable=SC2034
            local default_idx=0
            # shellcheck disable=SC2034
            while IFS='|' read -r id label cmd is_ai is_default; do
                [[ -z "$id" ]] && continue
                _AVAILABLE_CMD_LABELS+=("${label} (${cmd})")
                _AVAILABLE_CMD_VALUES+=("${cmd}")
                if [[ "$is_default" == "true" ]]; then
                    _AVAILABLE_CMD_DEFAULT_IDX="$i"
                fi
                i=$((i + 1))
            done <<< "$registry_out"
        fi
    fi

    # Fallback: if registry unavailable, fall back to hardcoded detection
    if [[ "$used_registry" == "false" ]]; then
        if command -v claude &>/dev/null; then
            _AVAILABLE_CMD_LABELS+=("Claude Code (claude --dangerously-skip-permissions)")
            _AVAILABLE_CMD_VALUES+=("claude --dangerously-skip-permissions")
            _AVAILABLE_CMD_DEFAULT_IDX=0
        fi

        if command -v codex &>/dev/null; then
            _AVAILABLE_CMD_LABELS+=("Codex ($(_i18n DEP_CODEX))")
            _AVAILABLE_CMD_VALUES+=("codex")
        fi

        if command -v opencode &>/dev/null; then
            _AVAILABLE_CMD_LABELS+=("OpenCode ($(_i18n DEP_OPENCODE))")
            _AVAILABLE_CMD_VALUES+=("opencode")
        fi

        if command -v lazygit &>/dev/null; then
            _AVAILABLE_CMD_LABELS+=("Lazygit")
            _AVAILABLE_CMD_VALUES+=("lazygit")
        fi

        if command -v yazi &>/dev/null; then
            _AVAILABLE_CMD_LABELS+=("Yazi ($(_i18n YAZI_DESC))")
            _AVAILABLE_CMD_VALUES+=("yazi")
        fi
    fi

    # Always append: empty shell + custom input
    _AVAILABLE_CMD_LABELS+=("$(_i18n EMPTY_SHELL_OPT)")
    _AVAILABLE_CMD_VALUES+=("__empty__")

    # Record the empty shell index (for use when needed)
    _AVAILABLE_CMD_EMPTY_IDX=$(( ${#_AVAILABLE_CMD_LABELS[@]} - 1 ))

    _AVAILABLE_CMD_LABELS+=("$(_i18n CUSTOM_INPUT)")
    _AVAILABLE_CMD_VALUES+=("__custom__")

    # Apply explicit default selector from tools.default
    if [[ "$_AVAILABLE_CMD_DEFAULT_REF" == "__empty__" ]]; then
        _AVAILABLE_CMD_DEFAULT_IDX="${_AVAILABLE_CMD_EMPTY_IDX:-0}"
    elif [[ "$_AVAILABLE_CMD_DEFAULT_REF" == "__custom__" ]]; then
        _AVAILABLE_CMD_DEFAULT_IDX=$(( ${#_AVAILABLE_CMD_LABELS[@]} - 1 ))
    fi
}

# Resolve default menu index for a target command.
# If multiple options share the same command string, prefer the globally
# configured default tool index.
# Args:
#   $1 target command string (empty/null means empty shell)
#   $2 fallback index (optional, default 0)
# Returns (stdout): resolved 0-based index
_pane_command_resolve_default_index() {
    local target_cmd="${1:-}"
    local fallback_idx="${2:-0}"

    if _is_null_or_empty "$target_cmd"; then
        echo "${_AVAILABLE_CMD_EMPTY_IDX:-$fallback_idx}"
        return 0
    fi

    local first_match_idx=""
    local i
    for i in "${!_AVAILABLE_CMD_VALUES[@]}"; do
        if [[ "${_AVAILABLE_CMD_VALUES[$i]}" == "$target_cmd" ]]; then
            [[ -z "$first_match_idx" ]] && first_match_idx="$i"
            if [[ "$i" == "${_AVAILABLE_CMD_DEFAULT_IDX:-0}" ]]; then
                echo "$i"
                return 0
            fi
        fi
    done

    if [[ -n "$first_match_idx" ]]; then
        echo "$first_match_idx"
    else
        echo "$fallback_idx"
    fi
}

# Resolve the currently configured default command into an executable command
# string, falling back to null for empty/custom sentinel values.
_pane_command_get_default_command() {
    local idx="${_AVAILABLE_CMD_DEFAULT_IDX:-0}"
    local value="${_AVAILABLE_CMD_VALUES[$idx]:-__empty__}"
    case "$value" in
        __empty__|__custom__|"") echo "null" ;;
        *) echo "$value" ;;
    esac
}

# ──────────────────────────────────────────────────
# Map a raw command string to a short display label.
# Uses tools registry if available, falls back to hardcoded map.
# $1 = command string (e.g. "claude --dangerously-skip-permissions")
# stdout: short label (e.g. "Claude Code"), or the original command if unknown
# ──────────────────────────────────────────────────
_cmd_display_label() {
    local cmd="$1"
    if _is_null_or_empty "$cmd"; then
        echo "$(_i18n EMPTY_SHELL)"
        return 0
    fi

    # Try tools registry first
    if check_cmd yq; then
        local registry_out
        registry_out=$(tools_registry_list 2>/dev/null || echo "")
        if [[ -n "$registry_out" ]]; then
            local _id _label _cmd _is_ai _is_default
            while IFS='|' read -r _id _label _cmd _is_ai _is_default; do
                [[ -z "$_id" ]] && continue
                if [[ "$_cmd" == "$cmd" ]]; then
                    echo "$_label"
                    return 0
                fi
            done <<< "$registry_out"
        fi
    fi

    # Fallback: hardcoded known commands
    case "$cmd" in
        claude*--dangerously-skip-permissions*) echo "Claude Code" ;;
        lazygit)  echo "Lazygit" ;;
        opencode) echo "OpenCode" ;;
        codex)    echo "Codex" ;;
        yazi)     echo "Yazi" ;;
        *)        echo "$cmd" ;;
    esac
}
