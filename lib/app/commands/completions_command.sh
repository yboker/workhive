#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Completions Command
# Shell completion script output & installation (App layer)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent double-loading
[[ -n "${_COMPLETIONS_COMMAND_SH_LOADED:-}" ]] && return 0
readonly _COMPLETIONS_COMMAND_SH_LOADED=1

completions_command_help() {
    cat <<EOF
Usage: $APP_NAME _internal_completions <shell>

Output shell completion script to stdout (internal command).

Subcommands:
  <shell>    Output completion script (zsh, bash, fish)

Supported shells: zsh, bash, fish
EOF
}

completions_command_execute() {
    local _completions_shell="${1:-}"
    case "$_completions_shell" in
        zsh)
            cat "$ROOT_DIR/completions/workhive.zsh"
            ;;
        bash)
            cat "$ROOT_DIR/completions/workhive.bash"
            ;;
        fish)
            cat "$ROOT_DIR/completions/workhive.fish"
            ;;
        --help|-h)
            completions_command_help
            ;;
        "")
            log_error "$(_i18n ERR_MISSING_ARG) <shell>"
            return 1
            ;;
        *)
            log_error "$(_i18n COMPLETIONS_ERR_UNSUPPORTED_SHELL) $_completions_shell"
            return 1
            ;;
    esac
}
