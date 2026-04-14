#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# AI Tool Adapter
# Detection and command building for AI coding tools (Infra layer)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[[ -n "${_AI_TOOL_ADAPTER_SH_LOADED:-}" ]] && return 0
readonly _AI_TOOL_ADAPTER_SH_LOADED=1

# Supported AI tools in priority order
readonly _AI_TOOL_PRIORITY=(claude codex opencode)

# Detect the best available AI coding tool.
# Returns (stdout): tool name or empty string.
ai_tool_detect() {
    for tool in "${_AI_TOOL_PRIORITY[@]}"; do
        if command -v "$tool" &>/dev/null; then
            echo "$tool"
            return 0
        fi
    done
    return 1
}

# Build the command string to launch an AI tool with a prompt.
# Args: $1 = tool name, $2 = prompt text
# Returns (stdout): shell command string
ai_tool_build_command() {
    local tool="$1"
    local prompt="$2"

    case "$tool" in
        claude)
            echo "claude -p '${prompt//\'/\\'\\'}' --dangerously-skip-permissions"
            ;;
        codex)
            echo "codex '${prompt//\'/\\'\\'}'"
            ;;
        opencode)
            echo "opencode"
            ;;
        custom)
            if [[ -n "${WORKHIVE_AI_COMMAND:-}" ]]; then
                echo "${WORKHIVE_AI_COMMAND} '${prompt//\'/\\'\\'}'"
            else
                echo "echo 'No custom AI command configured'"
            fi
            ;;
        *)
            echo "echo 'Unknown AI tool: $tool'"
            return 1
            ;;
    esac
}

# List all installed AI tools.
# Returns (stdout): one tool per line
ai_tool_list_installed() {
    for tool in "${_AI_TOOL_PRIORITY[@]}"; do
        if command -v "$tool" &>/dev/null; then
            echo "$tool"
        fi
    done
}
