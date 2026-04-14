#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# TUI Utils
# Terminal User Interface utility functions
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent double-loading
[[ -n "${_TUI_SH_LOADED:-}" ]] && return 0
readonly _TUI_SH_LOADED=1

# ANSI escape sequences
_TUI_RESET='\033[0m'
_TUI_BOLD='\033[1m'
_TUI_DIM='\033[2m'
_TUI_CYAN='\033[0;36m'
_TUI_GREEN='\033[0;32m'

# TUI output helper (forces output to /dev/tty to support command substitution capture)
# Safe printf usage: first argument is always treated as format string
_tui_print() {
    local fmt="$1"
    shift
    { printf "$fmt" "$@" > /dev/tty; } 2>/dev/null || true
}

# TUI plain text output (no format parsing, for text containing % and other special chars)
_tui_puts() {
    { printf '%s' "$1" > /dev/tty; } 2>/dev/null || true
}

# Cursor control
_cursor_hide() { _tui_print '\e[?25l'; }
_cursor_show() { _tui_print '\e[?25h'; }
_cursor_up() { _tui_print "\e[%sA" "${1:-1}"; }
_cursor_save() { _tui_print '\e7'; }
_cursor_restore() { _tui_print '\e8'; }
_cursor_clear_line() { _tui_print '\e[2K\r'; }
_cursor_erase_below() { _tui_print '\e[J'; }
# Unified TUI frame lifecycle.
# Default keeps main screen content visible.
# Optional alternate screen mode can be enabled with:
#   WORKHIVE_TUI_ALT_SCREEN=true
_TUI_FRAME_ACTIVE=0
_TUI_FRAME_USE_ALT=0
_TUI_FRAME_HAS_ANCHOR=0
_TUI_FRAME_REDRAW_MODE="lines" # lines|anchor
_tui_frame_enter() {
    if [[ "${WORKHIVE_TUI_ALT_SCREEN:-false}" == "true" ]]; then
        _tui_print '\e[?1049h\e[H'
        _TUI_FRAME_USE_ALT=1
        _TUI_FRAME_HAS_ANCHOR=0
        _TUI_FRAME_REDRAW_MODE="lines"
    else
        _TUI_FRAME_USE_ALT=0
        local redraw_mode="${WORKHIVE_TUI_REDRAW_MODE:-auto}"
        case "$redraw_mode" in
            anchor|lines) ;;
            auto|*) redraw_mode="auto" ;;
        esac

        # Auto mode: tmux panes prefer anchor mode (handles wrapped-line redraw drift);
        # plain terminals keep line-rewind mode for maximal compatibility.
        if [[ "$redraw_mode" == "auto" ]]; then
            if [[ -n "${TMUX:-}" ]]; then
                redraw_mode="anchor"
            else
                redraw_mode="lines"
            fi
        fi

        _TUI_FRAME_REDRAW_MODE="$redraw_mode"
        if [[ "$_TUI_FRAME_REDRAW_MODE" == "anchor" ]]; then
            _cursor_save
            _TUI_FRAME_HAS_ANCHOR=1
        else
            _TUI_FRAME_HAS_ANCHOR=0
        fi
    fi
    _TUI_FRAME_ACTIVE=1
}
_tui_frame_redraw() {
    if [[ "${_TUI_FRAME_USE_ALT:-0}" -eq 1 ]]; then
        _tui_print '\e[H\e[J'
    elif [[ "${_TUI_FRAME_REDRAW_MODE:-lines}" == "anchor" && "${_TUI_FRAME_HAS_ANCHOR:-0}" -eq 1 ]]; then
        _cursor_restore
        _cursor_erase_below
    else
        local lines="${_TUI_LAST_RENDER_LINES:-0}"
        if [[ "$lines" =~ ^[0-9]+$ ]] && (( lines > 0 )); then
            _cursor_up "$lines"
        fi
        _cursor_erase_below
    fi
}
_tui_frame_leave() {
    if [[ "${_TUI_FRAME_ACTIVE:-0}" -eq 1 ]]; then
        if [[ "${_TUI_FRAME_USE_ALT:-0}" -eq 1 ]]; then
            _tui_print '\e[?1049l'
        elif [[ "${_TUI_FRAME_REDRAW_MODE:-lines}" == "anchor" && "${_TUI_FRAME_HAS_ANCHOR:-0}" -eq 1 ]]; then
            _cursor_restore
            _cursor_erase_below
        else
            local lines="${_TUI_LAST_RENDER_LINES:-0}"
            if [[ "$lines" =~ ^[0-9]+$ ]] && (( lines > 0 )); then
                _cursor_up "$lines"
            fi
            _cursor_erase_below
        fi
        _TUI_FRAME_ACTIVE=0
        _TUI_FRAME_USE_ALT=0
        _TUI_FRAME_HAS_ANCHOR=0
        _TUI_FRAME_REDRAW_MODE="lines"
    fi
}

# Theme color definitions (RGB True Color)
# Format: \033[48;2;R;G;Bm (background) / \033[38;2;R;G;Bm (foreground)

# Catppuccin Mocha
readonly THEME_CATPPUCCIN_BG='\033[48;2;30;30;46m'       # #1e1e2e (Background)
readonly THEME_CATPPUCCIN_FG='\033[38;2;205;214;244m'     # #cdd6f4 (Text)
readonly THEME_CATPPUCCIN_ACCENT_BG='\033[48;2;137;180;250m' # #89b4fa (Blue Accent BG)
readonly THEME_CATPPUCCIN_ACCENT_FG='\033[38;2;30;30;46m'    # #1e1e2e (Dark Text on Accent)

# Tokyo Night
readonly THEME_TOKYO_BG='\033[48;2;26;27;38m'           # #1a1b26 (Background)
readonly THEME_TOKYO_FG='\033[38;2;192;202;245m'         # #c0caf5 (Text)
readonly THEME_TOKYO_ACCENT_BG='\033[48;2;122;162;247m'      # #7aa2f7 (Blue Accent BG)
readonly THEME_TOKYO_ACCENT_FG='\033[38;2;26;27;38m'         # #1a1b26 (Dark Text on Accent)

# Nord
readonly THEME_NORD_BG='\033[48;2;46;52;64m'            # #2e3440 (Background)
readonly THEME_NORD_FG='\033[38;2;216;222;233m'          # #d8dee9 (Text)
readonly THEME_NORD_ACCENT_BG='\033[48;2;136;192;208m'       # #88c0d0 (Cyan Accent BG)
readonly THEME_NORD_ACCENT_FG='\033[38;2;46;52;64m'          # #2e3440 (Dark Text on Accent)

# Gruvbox
readonly THEME_GRUVBOX_BG='\033[48;2;40;40;40m'          # #282828 (Background)
readonly THEME_GRUVBOX_FG='\033[38;2;235;219;178m'        # #ebdbb2 (Text)
readonly THEME_GRUVBOX_ACCENT_BG='\033[48;2;250;189;47m'     # #fabd2f (Yellow Accent BG)
readonly THEME_GRUVBOX_ACCENT_FG='\033[38;2;40;40;40m'       # #282828 (Dark Text on Accent)

# Dracula
readonly THEME_DRACULA_BG='\033[48;2;40;42;54m'           # #282a36 (Background)
readonly THEME_DRACULA_FG='\033[38;2;248;248;242m'         # #f8f8f2 (Text)
readonly THEME_DRACULA_ACCENT_BG='\033[48;2;189;147;249m'     # #bd93f9 (Purple Accent BG)
readonly THEME_DRACULA_ACCENT_FG='\033[38;2;40;42;54m'        # #282a36 (Dark Text on Accent)

# Rosé Pine
readonly THEME_ROSEPINE_BG='\033[48;2;25;23;36m'          # #191724 (Background)
readonly THEME_ROSEPINE_FG='\033[38;2;224;222;244m'        # #e0def4 (Text)
readonly THEME_ROSEPINE_ACCENT_BG='\033[48;2;196;167;231m'    # #c4a7e7 (Iris Accent BG)
readonly THEME_ROSEPINE_ACCENT_FG='\033[38;2;25;23;36m'       # #191724 (Dark Text on Accent)

# Everforest
readonly THEME_EVERFOREST_BG='\033[48;2;45;53;59m'         # #2d353b (Background)
readonly THEME_EVERFOREST_FG='\033[38;2;211;198;170m'       # #d3c6aa (Text)
readonly THEME_EVERFOREST_ACCENT_BG='\033[48;2;167;192;128m'   # #a7c080 (Green Accent BG)
readonly THEME_EVERFOREST_ACCENT_FG='\033[38;2;45;53;59m'      # #2d353b (Dark Text on Accent)

# One Dark
readonly THEME_ONEDARK_BG='\033[48;2;40;44;52m'           # #282c34 (Background)
readonly THEME_ONEDARK_FG='\033[38;2;171;178;191m'         # #abb2bf (Text)
readonly THEME_ONEDARK_ACCENT_BG='\033[48;2;97;175;239m'      # #61afef (Blue Accent BG)
readonly THEME_ONEDARK_ACCENT_FG='\033[38;2;40;44;52m'        # #282c34 (Dark Text on Accent)


# Render theme preview bar
# Args: $1 = theme identifier
_tui_render_theme_preview() {
    local theme="$1"
    local bg="" fg="" accent_bg="" accent_fg=""
    local theme_name=""

    case "$theme" in
        catppuccin-mocha)
            bg="$THEME_CATPPUCCIN_BG"
            fg="$THEME_CATPPUCCIN_FG"
            accent_bg="$THEME_CATPPUCCIN_ACCENT_BG"
            accent_fg="$THEME_CATPPUCCIN_ACCENT_FG"
            theme_name="Catppuccin Mocha"
            ;;
        tokyo-night)
            bg="$THEME_TOKYO_BG"
            fg="$THEME_TOKYO_FG"
            accent_bg="$THEME_TOKYO_ACCENT_BG"
            accent_fg="$THEME_TOKYO_ACCENT_FG"
            theme_name="Tokyo Night"
            ;;
        nord)
            bg="$THEME_NORD_BG"
            fg="$THEME_NORD_FG"
            accent_bg="$THEME_NORD_ACCENT_BG"
            accent_fg="$THEME_NORD_ACCENT_FG"
            theme_name="Nord"
            ;;
        gruvbox)
            bg="$THEME_GRUVBOX_BG"
            fg="$THEME_GRUVBOX_FG"
            accent_bg="$THEME_GRUVBOX_ACCENT_BG"
            accent_fg="$THEME_GRUVBOX_ACCENT_FG"
            theme_name="Gruvbox"
            ;;
        dracula)
            bg="$THEME_DRACULA_BG" fg="$THEME_DRACULA_FG"
            accent_bg="$THEME_DRACULA_ACCENT_BG" accent_fg="$THEME_DRACULA_ACCENT_FG"
            theme_name="Dracula"
            ;;
        rose-pine)
            bg="$THEME_ROSEPINE_BG" fg="$THEME_ROSEPINE_FG"
            accent_bg="$THEME_ROSEPINE_ACCENT_BG" accent_fg="$THEME_ROSEPINE_ACCENT_FG"
            theme_name="Rosé Pine"
            ;;
        everforest)
            bg="$THEME_EVERFOREST_BG" fg="$THEME_EVERFOREST_FG"
            accent_bg="$THEME_EVERFOREST_ACCENT_BG" accent_fg="$THEME_EVERFOREST_ACCENT_FG"
            theme_name="Everforest"
            ;;
        onedark)
            bg="$THEME_ONEDARK_BG" fg="$THEME_ONEDARK_FG"
            accent_bg="$THEME_ONEDARK_ACCENT_BG" accent_fg="$THEME_ONEDARK_ACCENT_FG"
            theme_name="One Dark"
            ;;
    esac

    # Simulate tmux status bar preview
    _tui_print "\n"
    local preview_label
    preview_label=$(_i18n PREVIEW)
    _tui_print "  %s %s\n" "$preview_label" "$theme_name"
    _tui_print "  ${accent_bg}${accent_fg}  workhive ${bg}${fg}  1:main*   2:lazygit ${bg}                          ${fg} user  10:00 ${_TUI_RESET}\n"
    _tui_print "\n"
}

# Total lines rendered last time (used for precise cursor rewind on redraw)
_TUI_LAST_RENDER_LINES=0

# Render menu (internal function)
# Sets _TUI_LAST_RENDER_LINES to the total lines rendered
_tui_render_menu() {
    local prompt="$1"
    local -n __render_opts_ref=$2  # Use a different name to avoid nameref circular reference
    local selected="$3"
    local callback="$4"
    local _line_count=0

    # Clear current line (prevent residual characters)
    _cursor_clear_line
    local nav_hint="Use Up/Down, Enter to confirm"
    if type -t _i18n &>/dev/null; then
        nav_hint=$(_i18n TUI_NAV)
    fi
    _tui_print "${_TUI_BOLD}${_TUI_CYAN}?${_TUI_RESET} %s ${_TUI_DIM}(%s)${_TUI_RESET}\n" "$prompt" "$nav_hint"
    _line_count=$((_line_count + 1))
    _cursor_clear_line
    _tui_print "\n"
    _line_count=$((_line_count + 1))

    # Print options (use %s to prevent % in option text from being parsed as format specifiers)
    local idx=0
    local opt
    for opt in "${__render_opts_ref[@]}"; do
        _cursor_clear_line
        if [[ "$idx" -eq "$selected" ]]; then
            _tui_print "  ${_TUI_GREEN}●${_TUI_RESET} %s\n" "$opt"
        else
            _tui_print "  ${_TUI_DIM}○ %s${_TUI_RESET}\n" "$opt"
        fi
        _line_count=$((_line_count + 1))
        idx=$((idx + 1))
    done

    # Invoke preview callback (callback line count returned via _TUI_CALLBACK_LINES)
    _TUI_CALLBACK_LINES=0
    if [[ -n "$callback" ]]; then
        "$callback" "$selected"
    fi
    _line_count=$((_line_count + _TUI_CALLBACK_LINES))

    _TUI_LAST_RENDER_LINES=$_line_count
}

# Generic single-select menu
# Args:
#   $1 - prompt message
#   $2 - options array variable name (e.g. "OPTIONS")
#   $3 - default index (0-based)
#   $4 - preview callback function name (optional, receives current selected index)
#   $5 - (deprecated, kept for compatibility) preview line count, no longer used
# Returns: selected index (0-based) via global variable TUI_SELECTED_INDEX (avoids subshell issues)
tui_select_menu() {
    local prompt="$1"
    local -n __tui_opts_ref=$2  # Pass-by-reference array (use unique name)
    local selected="${3:-0}"
    local callback="${4:-}"
    # $5 kept for compatibility, no longer used
    local key=""

    # Safety check: is /dev/tty available
    if ! [ -c /dev/tty ] 2>/dev/null; then
        # Non-TTY environment, use default value
        TUI_SELECTED_INDEX="$selected"
        return 0
    fi

    # Flush /dev/tty input buffer (prevent residual keystrokes from triggering immediate exit)
    while read -rsn1 -t 0.1 _discard < /dev/tty 2>/dev/null; do :; done

    # /dev/tty might exist but be unavailable for this process (e.g. detached shell)
    if ! { printf '' > /dev/tty; } 2>/dev/null; then
        TUI_SELECTED_INDEX="$selected"
        return 0
    fi

    # Enter unified TUI frame, then hide cursor.
    _tui_frame_enter
    _cursor_hide

    # Ctrl+C / SIGINT: always restore TUI frame + cursor before exit.
    trap '_cursor_show; _tui_frame_leave; printf "\n" > /dev/tty; trap - INT TERM EXIT; exit 130' INT
    trap '_cursor_show; _tui_frame_leave' TERM EXIT

    _TUI_LAST_RENDER_LINES=0
    _tui_frame_redraw
    _tui_render_menu "$prompt" __tui_opts_ref "$selected" "$callback"

    while true; do
        # Read keystroke (blocking)
        IFS= read -rsn1 key < /dev/tty

        # Handle escape sequences (arrow keys / ESC)
        if [[ "$key" == $'\x1b' ]]; then
            # Read subsequent sequence (non-blocking, short timeout; || true prevents exit under set -e)
            read -rsn2 -t 0.1 seq < /dev/tty || true
            case "$seq" in
                "[A") # Up
                    selected=$((selected - 1))
                    if ((selected < 0)); then selected=$((${#__tui_opts_ref[@]} - 1)); fi
                    _tui_frame_redraw
                    _tui_render_menu "$prompt" __tui_opts_ref "$selected" "$callback"
                    ;;
                "[B") # Down
                    selected=$((selected + 1))
                    if ((selected >= ${#__tui_opts_ref[@]})); then selected=0; fi
                    _tui_frame_redraw
                    _tui_render_menu "$prompt" __tui_opts_ref "$selected" "$callback"
                    ;;
                *)
                    # Bare ESC or unrecognized sequence → cancel selection
                    _cursor_show
                    _tui_frame_leave
                    trap - INT TERM EXIT
                    TUI_SELECTED_INDEX=-1
                    return 0
                    ;;
            esac
        elif [[ "$key" == "" ]]; then # Enter
            break
        fi
    done

    # Restore cursor and clear traps
    _cursor_show
    _tui_frame_leave
    trap - INT TERM EXIT

    # Return result
    TUI_SELECTED_INDEX="$selected"
    return 0
}

# ──────────────────────────────────────────────────
# Command selector (select + custom input)
# ──────────────────────────────────────────────────

# Command selector: choose from preset command list or enter custom input
# Args:
#   $1 - prompt message (e.g. "Select startup command for Pane 1")
#   $2 - option labels array variable name (display names, e.g. "CMD_LABELS")
#   $3 - option values array variable name (actual command values, e.g. "CMD_VALUES")
#   $4 - default index (0-based, default 0)
# Returns: selected command value via global variable TUI_SELECTED_CMD
#
# Note: last item in labels array should be "Custom input..." / "Empty Shell",
#       with corresponding values "__custom__" / "null"
tui_command_select() {
    local prompt="$1"
    local -n __cmd_labels_ref=$2
    local -n __cmd_values_ref=$3
    local default_idx="${4:-0}"

    # Use TUI select menu
    tui_select_menu "$prompt" __cmd_labels_ref "$default_idx" ""

    local selected_value="${__cmd_values_ref[$TUI_SELECTED_INDEX]}"

    if [[ "$selected_value" == "__custom__" ]]; then
        # Custom input mode
        local input_prompt="  Enter command> "
        if type -t _i18n &>/dev/null; then
            input_prompt="  $(_i18n INPUT_CMD) "
        fi
        local custom_cmd=""
        if [ -c /dev/tty ] 2>/dev/null && { printf '' > /dev/tty; } 2>/dev/null; then
            _tui_print "\n"
            read -p "$input_prompt" custom_cmd < /dev/tty || true
        fi
        if [[ -z "$custom_cmd" ]]; then
            # shellcheck disable=SC2034
            TUI_SELECTED_CMD="null"
        else
            # shellcheck disable=SC2034
            TUI_SELECTED_CMD="$custom_cmd"
        fi
    elif [[ "$selected_value" == "__empty__" ]]; then
        # shellcheck disable=SC2034
        TUI_SELECTED_CMD="null"
    else
        # shellcheck disable=SC2034
        TUI_SELECTED_CMD="$selected_value"
    fi

    return 0
}
