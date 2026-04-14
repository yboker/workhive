#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Process Capture
# Captures the full command line of running processes in tmux panes.
# Cross-platform: macOS (ps) and Linux (/proc).
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[[ -n "${_PERSISTENCE_PROCESS_CAPTURE_SH_LOADED:-}" ]] && return 0
readonly _PERSISTENCE_PROCESS_CAPTURE_SH_LOADED=1

# Default process restore list (programs that will be restarted on restore)
readonly _PERSISTENCE_DEFAULT_PROCESSES=":all:"

# ──────────────────────────────────────────────────
# Main: capture full command for a pane's child process
# $1 = pane PID (the shell process running in the pane)
# Returns (stdout): full command line, or empty if only a shell
# ──────────────────────────────────────────────────
process_capture_full_command() {
    local pane_pid="$1"
    [[ -z "$pane_pid" || "$pane_pid" == "0" ]] && return 0

    local child_pid
    child_pid=$(_capture_find_child_pid "$pane_pid")
    [[ -z "$child_pid" ]] && return 0

    local full_cmd
    full_cmd=$(_capture_get_cmdline "$child_pid")

    echo "$full_cmd"
}

# ──────────────────────────────────────────────────
# Find the primary child process of a pane shell
# $1 = parent PID (the shell)
# Returns: child PID or empty
# ──────────────────────────────────────────────────
_capture_find_child_pid() {
    local parent_pid="$1"

    if [[ "$(uname)" == "Linux" && -d "/proc/${parent_pid}/task" ]]; then
        # Linux: read child PID from /proc (children is a file, not a directory)
        local child children_file
        children_file="/proc/${parent_pid}/task/${parent_pid}/children"
        if [[ -f "$children_file" ]]; then
            child=$(tr ' ' '\n' < "$children_file" 2>/dev/null | head -n1)
        fi
        if [[ -z "$child" ]]; then
            child=$(pgrep -P "$parent_pid" 2>/dev/null | head -n1)
        fi
        echo "$child"
    else
        # macOS / fallback: use ps
        local child
        child=$(ps -o pid= -p "$(pgrep -P "$parent_pid" 2>/dev/null | head -n1)" 2>/dev/null | tr -d ' ')
        if [[ -z "$child" ]]; then
            child=$(pgrep -P "$parent_pid" 2>/dev/null | head -n1)
        fi
        echo "$child"
    fi
}

# ──────────────────────────────────────────────────
# Get the full command line for a PID
# Cross-platform implementation
# ──────────────────────────────────────────────────
_capture_get_cmdline() {
    local pid="$1"
    [[ -z "$pid" ]] && return 0

    if [[ "$(uname)" == "Linux" && -f "/proc/${pid}/cmdline" ]]; then
        # Linux: /proc gives NUL-separated args, properly escaped
        local cmd
        cmd=$(cat "/proc/${pid}/cmdline" 2>/dev/null | xargs -0 printf '%q ' 2>/dev/null)
        # Trim trailing space
        echo "${cmd% }"
    else
        # macOS: ps -o args= gives the full command line
        ps -p "$pid" -o args= 2>/dev/null | sed 's/^ *//'
    fi
}

# ──────────────────────────────────────────────────
# Check if a command should be restored
# $1 = command name (e.g. "vim", "lazygit")
# $2 = full command line (optional, for wrapper/interpreter detection)
# $3 = optional custom process list (space-separated)
# Returns: 0 = should restore, 1 = skip
# ──────────────────────────────────────────────────
process_should_restore() {
    local cmd_name="$1"
    local full_cmd="${2:-}"
    local process_list="${3:-}"

    if [[ -n "$full_cmd" ]]; then
        full_cmd=$(process_unwrap_restore_command "$full_cmd")
    fi
    if [[ -z "$cmd_name" && -n "$full_cmd" ]]; then
        cmd_name="${full_cmd%% *}"
    fi
    # Handle env-prefixed commands, e.g. "FOO=1 BAR=2 codex".
    if [[ "$cmd_name" =~ ^[A-Za-z_][A-Za-z0-9_]*=.*$ && -n "$full_cmd" ]]; then
        local probe="$full_cmd"
        local token rest
        while [[ -n "$probe" ]]; do
            token="${probe%% *}"
            if [[ "$token" =~ ^[A-Za-z_][A-Za-z0-9_]*=.*$ ]]; then
                rest="${probe#"$token"}"
                rest="${rest#"${rest%%[![:space:]]*}"}"
                probe="$rest"
                continue
            fi
            cmd_name="$token"
            break
        done
    fi

    [[ -z "$cmd_name" ]] && return 1
    cmd_name=$(basename "$cmd_name" 2>/dev/null || echo "$cmd_name")

    # Bare shells are never worth restoring (panes already start with a shell).
    # Shell wrappers (e.g. "bash -c vim", "zsh -lc codex") are handled below
    # in the whitelist path.
    local _has_shell_c_wrapper=false
    if [[ "$full_cmd" =~ [[:space:]]-[[:alnum:]]*c[[:alnum:]]*[[:space:]] ]]; then
        _has_shell_c_wrapper=true
    fi
    case "$cmd_name" in
        bash|zsh|sh|fish|dash|ksh|tcsh|csh)
            [[ -z "$full_cmd" || "$_has_shell_c_wrapper" != "true" ]] && return 1
            ;;
    esac

    if [[ -z "$process_list" ]]; then
        process_list=$(_persistence_get_process_list)
    fi

    # :all: restores every non-shell process
    [[ "$process_list" == ":all:" ]] && return 0

    # Shell wrappers: check if wrapping a restorable command (bash -c ...)
    case "$cmd_name" in
        bash|zsh|sh|fish|dash|ksh|tcsh|csh)
            local proc
            for proc in $process_list; do
                [[ "$proc" == "~"* ]] && continue
                [[ "$full_cmd" == *"$proc"* ]] && return 0
            done
            return 1
            ;;
    esac

    local proc
    for proc in $process_list; do
        if [[ "$proc" == "~"* ]]; then
            # Substring match
            local pattern="${proc#\~}"
            [[ "$cmd_name" == *"$pattern"* ]] && return 0
        else
            # Exact match (command name starts with)
            [[ "$cmd_name" == "$proc" || "$cmd_name" == "${proc} "* ]] && return 0
            # Interpreter-based tools (node, python, etc.): check full command
            if [[ -n "$full_cmd" && "$cmd_name" =~ ^(node|python|python3|ruby|perl|java)$ ]]; then
                [[ "$full_cmd" == *"/${proc} "* || "$full_cmd" == *"/${proc}" ]] && return 0
            fi
        fi
    done

    return 1
}

# Get the process restore list from config or defaults
_persistence_get_process_list() {
    local custom_list
    custom_list=$(_persistence_config_get "process_list" "")

    # Strip inline YAML comments (yq may preserve trailing "# ..." text)
    custom_list="${custom_list%%#*}"
    # Trim trailing whitespace left after comment removal
    custom_list="${custom_list%"${custom_list##*[![:space:]]}"}"

    if _is_null_or_empty "$custom_list" || [[ "$custom_list" == "null" || "$custom_list" == "[]" ]]; then
        echo "$_PERSISTENCE_DEFAULT_PROCESSES"
    else
        # YAML array → space-separated string
        local _tmp="${custom_list//[/}"
        _tmp="${_tmp//]/}"
        _tmp="${_tmp//\"/}"
        _tmp="${_tmp//,/ }"
        read -r _tmp <<< "$_tmp"
        echo "$_tmp"
    fi
}

# ──────────────────────────────────────────────────
# Resolve the restore command for a program
# Some programs need special handling (e.g. vim -S for sessions)
# $1 = original command, $2 = working directory
# Returns: command to execute for restoration
# ──────────────────────────────────────────────────
process_resolve_restore_command() {
    local original_cmd="$1"
    local workdir="$2"

    [[ -z "$original_cmd" ]] && return 0
    original_cmd=$(process_unwrap_restore_command "$original_cmd")
    [[ -z "$original_cmd" ]] && return 0

    local cmd_name
    cmd_name="${original_cmd%% *}"
    if [[ "$cmd_name" =~ ^[A-Za-z_][A-Za-z0-9_]*=.*$ ]]; then
        local probe="$original_cmd"
        local token rest
        while [[ -n "$probe" ]]; do
            token="${probe%% *}"
            if [[ "$token" =~ ^[A-Za-z_][A-Za-z0-9_]*=.*$ ]]; then
                rest="${probe#"$token"}"
                rest="${rest#"${rest%%[![:space:]]*}"}"
                probe="$rest"
                continue
            fi
            cmd_name="$token"
            break
        done
    fi
    # Strip path prefix to get bare command name
    cmd_name=$(basename "$cmd_name" 2>/dev/null || echo "$cmd_name")

    case "$cmd_name" in
        vim|nvim)
            if [[ -f "${workdir}/Session.vim" ]]; then
                echo "${cmd_name} -S"
            else
                echo "$original_cmd"
            fi
            ;;
        *)
            echo "$original_cmd"
            ;;
    esac
}

# ──────────────────────────────────────────────────
# Classify the process into a restore strategy
# $1 = command name
# $2 = full command line (optional)
# Returns (stdout): "shell", "tui", "ai", or "generic"
# ──────────────────────────────────────────────────
process_classify_restore_strategy() {
    local cmd_name="$1"
    local full_cmd="${2:-}"

    if [[ -n "$full_cmd" ]]; then
        full_cmd=$(process_unwrap_restore_command "$full_cmd")
    fi
    if [[ -z "$cmd_name" && -n "$full_cmd" ]]; then
        cmd_name="${full_cmd%% *}"
        cmd_name=$(basename "$cmd_name" 2>/dev/null || echo "$cmd_name")
    fi
    if [[ "$cmd_name" =~ ^[A-Za-z_][A-Za-z0-9_]*=.*$ && -n "$full_cmd" ]]; then
        local probe="$full_cmd"
        local token rest
        while [[ -n "$probe" ]]; do
            token="${probe%% *}"
            if [[ "$token" =~ ^[A-Za-z_][A-Za-z0-9_]*=.*$ ]]; then
                rest="${probe#"$token"}"
                rest="${rest#"${rest%%[![:space:]]*}"}"
                probe="$rest"
                continue
            fi
            cmd_name="$token"
            break
        done
        cmd_name=$(basename "$cmd_name" 2>/dev/null || echo "$cmd_name")
    fi
    [[ -z "$cmd_name" ]] && return 0

    case "$cmd_name" in
        bash|zsh|sh|fish|dash|ksh|tcsh|csh)
            echo "shell"
            ;;
        vi|vim|nvim|emacs|nano|less|more|tail|top|htop|lazygit|k9s|yazi|mc|ranger)
            echo "tui"
            ;;
        claude|opencode|codex)
            echo "ai"
            ;;
        *)
            echo "generic"
            ;;
    esac
}

# Sanitize a command string for safe logging.
# Records only the command name and argument count to avoid leaking
# secrets and to sidestep word-splitting issues with quoted arguments.
# $1 = full command string
# Returns: "cmd_name" (no args) or "cmd_name (...)" (has args)
process_sanitize_for_log() {
    local cmd="$1"
    [[ -z "$cmd" ]] && return 0

    local cmd_name="${cmd%% *}"
    local rest="${cmd#"$cmd_name"}"

    if [[ -z "${rest// /}" ]]; then
        echo "$cmd_name"
    else
        echo "${cmd_name} (...)"
    fi
}

# Delegates to _has_shell_metacharacters (common.sh).
# Returns: 0 = clean, 1 = contains shell metacharacters
_process_has_injection_chars() {
    _has_shell_metacharacters "$1"
}

# Unwrap tmux respawn wrapper command back to the original startup command.
# The wrapper shape is produced by terminal_adapter_spawn_command_target:
#   <shell> -lc '<runner_script>' _ <command> <shell>
# Returns: original command if wrapper matched; otherwise input command.
process_unwrap_restore_command() {
    local cmd="$1"
    [[ -z "$cmd" ]] && return 0

    # Fast path for normal commands.
    if [[ "$cmd" != *'eval -- "$1"'* || "$cmd" != *'exec "$2" -l'* || "$cmd" != *' _ '* ]]; then
        echo "$cmd"
        return 0
    fi

    local tail="${cmd##* _ }"
    [[ -n "$tail" ]] || {
        echo "$cmd"
        return 0
    }

    local shell_token="${tail##* }"
    local wrapped_cmd="${tail% ${shell_token}}"
    local shell_name
    shell_name=$(basename "$shell_token" 2>/dev/null || echo "$shell_token")

    case "$shell_name" in
        bash|zsh|sh|fish|dash|ksh|tcsh|csh)
            if [[ -n "$wrapped_cmd" && "$wrapped_cmd" != "$tail" ]]; then
                # startup command may be prefixed by env assignments:
                #   FOO=1 BAR=2 codex
                # Strip leading KEY=VALUE tokens so restore matching uses real command.
                local cleaned="$wrapped_cmd"
                local token rest
                while [[ -n "$cleaned" ]]; do
                    token="${cleaned%% *}"
                    if [[ "$token" =~ ^[A-Za-z_][A-Za-z0-9_]*=.*$ ]]; then
                        rest="${cleaned#"$token"}"
                        rest="${rest#"${rest%%[![:space:]]*}"}"
                        cleaned="$rest"
                        continue
                    fi
                    break
                done
                [[ -n "$cleaned" ]] && wrapped_cmd="$cleaned"
                echo "$wrapped_cmd"
                return 0
            fi
            ;;
    esac

    echo "$cmd"
}

# Validate that a restore command is safe to execute.
# All strategies (including tui/ai) are checked for injection metacharacters
# to prevent snapshot tampering attacks.
# $1 = full command string
# Returns: 0 = safe, 1 = rejected
process_validate_restore_command() {
    local cmd="$1"
    [[ -z "$cmd" ]] && return 1

    cmd=$(process_unwrap_restore_command "$cmd")
    [[ -z "$cmd" ]] && return 1

    _process_has_injection_chars "$cmd" || return 1

    return 0
}
