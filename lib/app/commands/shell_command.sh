#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Shell Command
# Unified shell integration management (App layer)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[[ -n "${_SHELL_COMMAND_SH_LOADED:-}" ]] && return 0
readonly _SHELL_COMMAND_SH_LOADED=1

source "${LIB_DIR}/utils/i18n.sh"

shell_command_help() {
    _help_dispatch "_shell_command_help"
}

_SHELL_MARKER="# workhive Shell Integration"
_SHELL_BLOCK_BEGIN="# >>> workhive shell integration >>>"
_SHELL_BLOCK_END="# <<< workhive shell integration <<<"
_SHELL_CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/workhive/shell"
_SHELL_MODULES_SUPPORTED="completions,aliases,history,zoxide,starship,ssh-term-fix"

_shell_detect() {
    echo "${SHELL##*/}"
}

_shell_get_rc_file() {
    case "$1" in
        zsh)  echo "$HOME/.zshrc" ;;
        bash)
            [[ "$(uname -s)" == "Darwin" ]] && echo "$HOME/.bash_profile" || echo "$HOME/.bashrc"
            ;;
        *)    echo "" ;;
    esac
}

_shell_get_installed_path() {
    case "$1" in
        zsh)  echo "${_SHELL_CONFIG_DIR}/workhive.zsh" ;;
        bash) echo "${_SHELL_CONFIG_DIR}/workhive.bash" ;;
        *)    echo "" ;;
    esac
}

_shell_source_line() {
    local integration_path="$1"
    echo "[[ -f \"$integration_path\" ]] && source \"$integration_path\" $_SHELL_MARKER"
}

_shell_can_prompt() {
    [[ -c /dev/tty ]] && [[ -r /dev/tty ]] && [[ -w /dev/tty ]]
}

_shell_profile_modules() {
    case "$1" in
        minimal)  echo "completions" ;;
        standard) echo "completions,aliases,history" ;;
        advanced) echo "completions,aliases,history,zoxide,starship,ssh-term-fix" ;;
        *)        echo "completions" ;;
    esac
}

_shell_profile_desc() {
    case "$1" in
        minimal)  echo "$(_i18n SHELL_PROFILE_MINIMAL_DESC)" ;;
        standard) echo "$(_i18n SHELL_PROFILE_STANDARD_DESC)" ;;
        advanced) echo "$(_i18n SHELL_PROFILE_ADVANCED_DESC)" ;;
        *)        echo "$(_i18n SHELL_PROFILE_UNKNOWN)" ;;
    esac
}

_shell_module_desc() {
    case "$1" in
        completions)  echo "$(_i18n SHELL_MODULE_COMPLETIONS_DESC)" ;;
        aliases)      echo "$(_i18n SHELL_MODULE_ALIASES_DESC)" ;;
        history)      echo "$(_i18n SHELL_MODULE_HISTORY_DESC)" ;;
        zoxide)       echo "$(_i18n SHELL_MODULE_ZOXIDE_DESC)" ;;
        starship)     echo "$(_i18n SHELL_MODULE_STARSHIP_DESC)" ;;
        ssh-term-fix) echo "$(_i18n SHELL_MODULE_SSH_TERM_FIX_DESC)" ;;
        *)            echo "$(_i18n SHELL_MODULE_UNKNOWN_DESC)" ;;
    esac
}

_shell_module_dep_cmd() {
    case "$1" in
        zoxide)   echo "zoxide" ;;
        starship) echo "starship" ;;
        *)        echo "" ;;
    esac
}

_shell_module_install_hint() {
    case "$1" in
        zoxide)   echo "brew install zoxide" ;;
        starship) echo "brew install starship / curl -sS https://starship.rs/install.sh | sh" ;;
        *)        echo "" ;;
    esac
}

_shell_prompt_module_dep_note() {
    local module="$1"
    local dep_cmd="" hint=""
    dep_cmd=$(_shell_module_dep_cmd "$module")
    [[ -z "$dep_cmd" ]] && return 0

    if command -v "$dep_cmd" >/dev/null 2>&1; then
        echo "$(_i18n_fmt SHELL_MODULE_DEP_INSTALLED_FMT "$dep_cmd")" > /dev/tty
        return 0
    fi

    echo "$(_i18n_fmt SHELL_MODULE_DEP_MISSING_FMT "$dep_cmd")" > /dev/tty
    hint=$(_shell_module_install_hint "$module")
    [[ -n "$hint" ]] && echo "$(_i18n_fmt SHELL_MODULE_DEP_INSTALL_HINT_FMT "$hint")" > /dev/tty
}

_shell_report_missing_module_deps() {
    local modules_csv="$1"
    local module="" dep_cmd="" hint="" missing=false
    local modules_order=("completions" "aliases" "history" "zoxide" "starship" "ssh-term-fix")

    for module in "${modules_order[@]}"; do
        _shell_csv_has "$modules_csv" "$module" || continue
        dep_cmd=$(_shell_module_dep_cmd "$module")
        [[ -z "$dep_cmd" ]] && continue
        command -v "$dep_cmd" >/dev/null 2>&1 && continue

        if [[ "$missing" == false ]]; then
            log_info "$(_i18n SHELL_MODULE_MISSING_DEPS_HEADER)"
            missing=true
        fi
        hint=$(_shell_module_install_hint "$module")
        log_info "  $(_i18n_fmt SHELL_MODULE_MISSING_DEPS_ITEM_FMT "$module" "$dep_cmd" "$hint")"
    done
}

_shell_csv_has() {
    local csv="$1" item="$2"
    case ",$csv," in
        *,"$item",*) return 0 ;;
        *) return 1 ;;
    esac
}

_shell_csv_add_unique() {
    local csv="$1" item="$2"
    if [[ -z "$csv" ]]; then
        echo "$item"
        return 0
    fi
    if _shell_csv_has "$csv" "$item"; then
        echo "$csv"
    else
        echo "$csv,$item"
    fi
}

_shell_csv_remove() {
    local csv="$1" item="$2"
    local out="" part
    IFS=',' read -r -a _parts <<< "$csv"
    for part in "${_parts[@]}"; do
        [[ -z "$part" || "$part" == "$item" ]] && continue
        if [[ -z "$out" ]]; then
            out="$part"
        else
            out="$out,$part"
        fi
    done
    echo "$out"
}

_shell_normalize_modules() {
    local csv="$1"
    local out="" part
    IFS=',' read -r -a _parts <<< "$csv"
    for part in "${_parts[@]}"; do
        part="${part#"${part%%[![:space:]]*}"}"
        part="${part%"${part##*[![:space:]]}"}"
        [[ -z "$part" ]] && continue
        if ! _shell_csv_has "$_SHELL_MODULES_SUPPORTED" "$part"; then
            log_error "$(_i18n_fmt SHELL_ERR_UNKNOWN_MODULE_FMT "$part")"
            return 1
        fi
        out=$(_shell_csv_add_unique "$out" "$part")
    done
    echo "$out"
}

_shell_prompt_profile() {
    _SHELL_PROMPT_PROFILE_RESULT="minimal"

    if ! [ -t 0 ] || ! [ -t 1 ]; then
        return 0
    fi

    echo ""
    echo "$(_i18n SHELL_SELECT_PROFILE)"
    echo "  1) minimal   ($(_i18n SHELL_RECOMMENDED_DEFAULT))"
    echo "  2) standard"
    echo "  3) advanced"
    echo ""
    local ans=""
    while true; do
        read -r -p "$(_i18n SHELL_SELECT_PROFILE_CHOOSE) " ans < /dev/tty
        [[ -z "$ans" ]] && ans="1"
        case "$ans" in
            1) _SHELL_PROMPT_PROFILE_RESULT="minimal"; return 0 ;;
            2) _SHELL_PROMPT_PROFILE_RESULT="standard"; return 0 ;;
            3) _SHELL_PROMPT_PROFILE_RESULT="advanced"; return 0 ;;
            *) echo "$(_i18n SHELL_INVALID_CHOICE)" ;;
        esac
    done
}

_shell_prompt_profile_guided() {
    if ! _shell_can_prompt; then
        echo "minimal"
        return 0
    fi

    echo "" > /dev/tty
    echo "$(_shell_profile_desc "minimal")" > /dev/tty
    if ask_yes_no "$(_i18n SHELL_ASK_USE_PROFILE_MINIMAL)" "Y" > /dev/tty; then
        echo "minimal"
        return 0
    fi

    echo "" > /dev/tty
    echo "$(_shell_profile_desc "standard")" > /dev/tty
    if ask_yes_no "$(_i18n SHELL_ASK_USE_PROFILE_STANDARD)" "Y" > /dev/tty; then
        echo "standard"
        return 0
    fi

    echo "" > /dev/tty
    echo "$(_shell_profile_desc "advanced")" > /dev/tty
    if ask_yes_no "$(_i18n SHELL_ASK_USE_PROFILE_ADVANCED)" "Y" > /dev/tty; then
        echo "advanced"
        return 0
    fi

    # Fallback if user declines all.
    echo "minimal"
}

_shell_prompt_modules_guided() {
    local modules_csv="$1"
    local out="$modules_csv"
    local module=""
    local default_ans=""
    local modules_order=("completions" "aliases" "history" "zoxide" "starship" "ssh-term-fix")

    _SHELL_PROMPT_MODULES_RESULT="$out"

    if ! _shell_can_prompt; then
        return 0
    fi

    if ! ask_yes_no "$(_i18n SHELL_ASK_CUSTOMIZE_MODULES)" "N"; then
        _SHELL_PROMPT_MODULES_RESULT="$out"
        return 0
    fi

    for module in "${modules_order[@]}"; do
        echo ""
        echo "$(_i18n_fmt SHELL_MODULE_DESC_FMT "$module" "$(_shell_module_desc "$module")")"
        _shell_prompt_module_dep_note "$module"
        if _shell_csv_has "$out" "$module"; then
            default_ans="Y"
        else
            default_ans="N"
        fi
        if [[ "$default_ans" == "Y" ]]; then
            local dep_cmd=""
            dep_cmd=$(_shell_module_dep_cmd "$module")
            if [[ -n "$dep_cmd" ]] && ! command -v "$dep_cmd" >/dev/null 2>&1; then
                default_ans="N"
            fi
        fi

        if ask_yes_no "$(_i18n_fmt SHELL_ASK_ENABLE_MODULE_FMT "$module")" "$default_ans"; then
            out=$(_shell_csv_add_unique "$out" "$module")
        else
            out=$(_shell_csv_remove "$out" "$module")
        fi
    done

    _SHELL_PROMPT_MODULES_RESULT="$out"
}

_shell_should_prompt_modules() {
    local non_interactive="$1"
    local normalized_enable="$2"
    local normalized_disable="$3"

    [[ "$non_interactive" == true ]] && return 1

    # When modules are explicitly provided by flags, honor them directly
    # instead of asking interactive module-by-module customization.
    if [[ -n "$normalized_enable" || -n "$normalized_disable" ]]; then
        return 1
    fi

    return 0
}

_shell_append_if_missing() {
    local file="$1" line="$2"
    if [[ -f "$file" ]] && grep -qF "$line" "$file" 2>/dev/null; then
        return 0
    fi
    echo "" >> "$file"
    echo "$line" >> "$file"
}

_shell_remove_integration_lines() {
    local rc_file="$1"
    local tmp_file="${rc_file}.workhive_bak"
    # Remove new block markers, legacy marker lines, and blank lines just before removed lines.
    awk -v marker="$_SHELL_MARKER" -v begin="$_SHELL_BLOCK_BEGIN" -v end="$_SHELL_BLOCK_END" '
        {
            marker_l = tolower(marker)
            if (in_block == 1) {
                if ($0 == end) { in_block = 0 }
                next
            }
            if ($0 == begin) { in_block = 1; next }
            if (index(tolower($0), marker_l) > 0) {
                while (n > 0 && out[n] == "") n--
                skip_blank_after = 1
                next
            }
            if (skip_blank_after == 1 && $0 == "") {
                next
            }
            skip_blank_after = 0
            out[++n] = $0
        }
        END {
            for (i = 1; i <= n; i++) print out[i]
        }
    ' "$rc_file" > "$tmp_file"
    mv -- "$tmp_file" "$rc_file"
}

_shell_append_managed_block() {
    local rc_file="$1" source_line="$2"
    {
        echo "$_SHELL_BLOCK_BEGIN"
        echo "$source_line"
        echo "$_SHELL_BLOCK_END"
    } >> "$rc_file"
}

_shell_render_completions_module() {
    local target_shell="$1"
    cat <<EOF
# Module: completions
if command -v workhive >/dev/null 2>&1; then
    eval "\$(workhive _internal_completions ${target_shell} 2>/dev/null)"
fi
EOF
}

_shell_render_zoxide_module() {
    local target_shell="$1"
    cat <<EOF
# Module: zoxide
if command -v zoxide >/dev/null 2>&1; then
    eval "\$(zoxide init ${target_shell})"
fi
EOF
}

_shell_render_starship_module() {
    local target_shell="$1"
    cat <<EOF
# Module: starship
if [[ -z "\${STARSHIP_SHELL:-}" ]] && command -v starship >/dev/null 2>&1; then
    eval "\$(starship init ${target_shell})"
fi
EOF
}

_shell_render_script() {
    local target_shell="$1" modules_csv="$2"

    cat <<EOF
# workhive ${target_shell} integration - generated by workhive shell setup
# Profile modules: ${modules_csv}
# Managed file. Manual edits may be overwritten.

EOF

    case "$target_shell" in
        zsh)
            cat <<'EOF'
[[ -n "${_WORKHIVE_ZSH_LOADED:-}" ]] && return 0
_WORKHIVE_ZSH_LOADED=1
setopt interactive_comments
bindkey -e
EOF
            ;;
        bash)
            cat <<'EOF'
[[ -n "${_WORKHIVE_BASH_LOADED:-}" ]] && return 0
_WORKHIVE_BASH_LOADED=1
shopt -s checkwinsize
EOF
            ;;
        *)
            return 1
            ;;
    esac

    if _shell_csv_has "$modules_csv" "completions"; then
        _shell_render_completions_module "$target_shell"
    fi

    if _shell_csv_has "$modules_csv" "aliases"; then
        cat <<'EOF'
# Module: aliases
alias ll='ls -lhF'
alias la='ls -lAhF'
alias l='ls -CF'
alias g='git'
alias gst='git status'
EOF
    fi

    if _shell_csv_has "$modules_csv" "history"; then
        case "$target_shell" in
            zsh)
                cat <<'EOF'
# Module: history
HISTSIZE="${HISTSIZE:-50000}"
SAVEHIST="${SAVEHIST:-50000}"
if [[ -z "${HISTFILE:-}" ]]; then
    HISTFILE="${ZDOTDIR:-$HOME}/.zsh_history"
fi
setopt HIST_FIND_NO_DUPS
setopt SHARE_HISTORY
setopt APPEND_HISTORY
setopt INC_APPEND_HISTORY
setopt EXTENDED_HISTORY
EOF
                ;;
            bash)
                cat <<'EOF'
# Module: history
HISTSIZE="${HISTSIZE:-50000}"
HISTFILESIZE="${HISTFILESIZE:-50000}"
shopt -s histappend
PROMPT_COMMAND="history -a; history -n; ${PROMPT_COMMAND:-:}"
EOF
                ;;
        esac
    fi

    if _shell_csv_has "$modules_csv" "zoxide"; then
        _shell_render_zoxide_module "$target_shell"
    fi

    if _shell_csv_has "$modules_csv" "starship"; then
        _shell_render_starship_module "$target_shell"
    fi

    if _shell_csv_has "$modules_csv" "ssh-term-fix"; then
        cat <<'EOF'
# Module: ssh-term-fix
if [[ "${TERM:-}" == "tmux-256color" ]]; then
    ssh() { TERM=xterm-256color command ssh "$@"; }
fi
EOF
    fi
}

shell_command_execute() {
    local subcmd="${1:-}"
    [[ -n "$subcmd" ]] && shift

    case "$subcmd" in
        -h|--help|"")
            shell_command_help
            return 0
            ;;
        setup|init) _shell_cmd_setup "$@" ;;
        remove) _shell_cmd_remove "$@" ;;
        show)   _shell_cmd_show "$@" ;;
        *)
            log_error "$(_i18n ERR_UNKNOWN_CMD) $subcmd"
            echo ""
            shell_command_help
            return 1
            ;;
    esac
}

_shell_cmd_setup() {
    local target_shell="" dry_run=false
    local profile="" enable_csv="" disable_csv=""
    local non_interactive=false

    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help) shell_command_help; return 0 ;;
            --shell)
                target_shell=$(cli_require_option_value "--shell" "${2:-}") || return 1
                shift 2
                ;;
            --profile)
                profile=$(cli_require_option_value "--profile" "${2:-}") || return 1
                shift 2
                ;;
            --enable)
                enable_csv=$(cli_require_option_value "--enable" "${2:-}") || return 1
                shift 2
                ;;
            --disable)
                disable_csv=$(cli_require_option_value "--disable" "${2:-}") || return 1
                shift 2
                ;;
            --yes) non_interactive=true; shift ;;
            --dry-run) dry_run=true; shift ;;
            -*)
                cli_log_unknown_opt "$1"
                return 1
                ;;
            *)
                cli_log_no_positional "shell setup"
                return 1
                ;;
        esac
    done

    [[ -z "$target_shell" ]] && target_shell=$(_shell_detect)
    if [[ -z "$profile" ]]; then
        if [[ "$non_interactive" == true ]]; then
            profile="minimal"
        else
            _shell_prompt_profile
            profile="${_SHELL_PROMPT_PROFILE_RESULT:-minimal}"
        fi
    fi
    case "$profile" in
        minimal|standard|advanced) ;;
        *)
            log_error "$(_i18n_fmt SHELL_ERR_INVALID_PROFILE_FMT "$profile")"
            return 1
            ;;
    esac

    local rc_file dst_path source_line
    rc_file=$(_shell_get_rc_file "$target_shell")
    dst_path=$(_shell_get_installed_path "$target_shell")
    source_line=$(_shell_source_line "$dst_path")

    if [[ -z "$rc_file" || -z "$dst_path" ]]; then
        log_error "$(_i18n SHELL_UNSUPPORTED) $target_shell"
        return 1
    fi

    local modules_csv
    modules_csv=$(_shell_profile_modules "$profile")

    local normalized_enable normalized_disable part
    normalized_enable=$(_shell_normalize_modules "$enable_csv") || return 1
    normalized_disable=$(_shell_normalize_modules "$disable_csv") || return 1

    if [[ -n "$normalized_enable" ]]; then
        IFS=',' read -r -a _en_parts <<< "$normalized_enable"
        for part in "${_en_parts[@]}"; do
            modules_csv=$(_shell_csv_add_unique "$modules_csv" "$part")
        done
    fi
    if [[ -n "$normalized_disable" ]]; then
        IFS=',' read -r -a _dis_parts <<< "$normalized_disable"
        for part in "${_dis_parts[@]}"; do
            modules_csv=$(_shell_csv_remove "$modules_csv" "$part")
        done
    fi

    if _shell_should_prompt_modules "$non_interactive" "$normalized_enable" "$normalized_disable"; then
        _shell_prompt_modules_guided "$modules_csv"
        modules_csv="${_SHELL_PROMPT_MODULES_RESULT:-$modules_csv}"
    fi

    if [[ "$dry_run" == true ]]; then
        echo ""
        log_info "$(_i18n SHELL_DRY_RUN_HEADER)"
        echo "  $(_i18n SHELL_LABEL_PROFILE): $profile"
        echo "  $(_i18n SHELL_LABEL_MODULES): ${modules_csv:-$(_i18n SHELL_LABEL_NONE)}"
        echo "  $(_i18n SHELL_INSTALL_TO): $dst_path"
        echo "  $(_i18n SHELL_TARGET_FILE): $rc_file"
        echo "  $(_i18n SHELL_LINE_TO_ADD):"
        echo "    $source_line"
        _shell_report_missing_module_deps "$modules_csv"
        echo ""
        return 0
    fi

    mkdir -p "${dst_path%/*}"
    _shell_render_script "$target_shell" "$modules_csv" > "$dst_path"
    chmod 644 "$dst_path"
    touch "$rc_file"

    if [[ -f "$rc_file" ]] && grep -qi "workhive shell integration" "$rc_file" 2>/dev/null; then
        _shell_remove_integration_lines "$rc_file"
    fi
    _shell_append_managed_block "$rc_file" "$source_line"

    echo ""
    log_success "$(_i18n SHELL_INIT_SUCCESS)"
    log_info "$(_i18n SHELL_LABEL_PROFILE): $profile"
    log_info "$(_i18n SHELL_LABEL_MODULES): ${modules_csv:-$(_i18n SHELL_LABEL_NONE)}"
    log_info "$(_i18n SHELL_INSTALL_TO): $dst_path"
    log_info "$(_i18n SHELL_TARGET_FILE): $rc_file"
    _shell_report_missing_module_deps "$modules_csv"
    echo ""
    log_info "$(_i18n SHELL_RESTART_HINT)"
    echo "  exec $target_shell -l"
    echo ""
    return 0
}

_shell_cmd_remove() {
    local target_shell="" dry_run=false

    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)  shell_command_help; return 0 ;;
            --shell)
                target_shell=$(cli_require_option_value "--shell" "${2:-}") || return 1
                shift 2
                ;;
            --dry-run) dry_run=true; shift ;;
            -*)
                cli_log_unknown_opt "$1"
                return 1
                ;;
            *)
                cli_log_no_positional "shell remove"
                return 1
                ;;
        esac
    done

    [[ -z "$target_shell" ]] && target_shell=$(_shell_detect)

    local rc_file
    rc_file=$(_shell_get_rc_file "$target_shell")

    if [[ -z "$rc_file" || ! -f "$rc_file" ]]; then
        log_info "$(_i18n SHELL_NOT_INSTALLED)"
        return 0
    fi

    if ! grep -qi "workhive shell integration" "$rc_file"; then
        log_info "$(_i18n SHELL_NOT_INSTALLED)"
        return 0
    fi

    if [[ "$dry_run" == true ]]; then
        echo ""
        log_info "$(_i18n SHELL_DRY_RUN_HEADER)"
        echo "  $(_i18n SHELL_REMOVE_FROM): $rc_file"
        grep -ni "workhive shell integration" "$rc_file" | while read -r line; do
            echo "  - $line"
        done
        echo ""
        return 0
    fi

    _shell_remove_integration_lines "$rc_file"

    local installed_path
    installed_path=$(_shell_get_installed_path "$target_shell")
    if [[ -n "$installed_path" && -f "$installed_path" ]]; then
        rm -f -- "$installed_path"
    fi
    if [[ -d "$_SHELL_CONFIG_DIR" ]] && [[ -z "$(find "$_SHELL_CONFIG_DIR" -mindepth 1 -maxdepth 1 2>/dev/null)" ]]; then
        rmdir "$_SHELL_CONFIG_DIR" 2>/dev/null || true
    fi

    echo ""
    log_success "$(_i18n SHELL_REMOVE_SUCCESS)"
    log_info "$(_i18n SHELL_RESTART_HINT)"
    echo "  exec $target_shell -l"
    echo ""
    return 0
}

_shell_cmd_show() {
    local target_shell=""

    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help) shell_command_help; return 0 ;;
            --shell)
                target_shell=$(cli_require_option_value "--shell" "${2:-}") || return 1
                shift 2
                ;;
            -*)
                cli_log_unknown_opt "$1"
                return 1
                ;;
            *)
                cli_log_no_positional "shell show"
                return 1
                ;;
        esac
    done

    [[ -z "$target_shell" ]] && target_shell=$(_shell_detect)

    local dst_path
    dst_path=$(_shell_get_installed_path "$target_shell")

    if [[ -n "$dst_path" && -f "$dst_path" ]]; then
        cat "$dst_path"
    else
        _shell_render_script "$target_shell" "$(_shell_profile_modules minimal)"
    fi
}
