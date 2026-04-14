#!/usr/bin/env bash
# Shared theme definitions

# Source guard: prevent duplicate loading
[[ -n "${_THEME_HELPER_SH_LOADED:-}" ]] && return 0
readonly _THEME_HELPER_SH_LOADED=1

# Dependencies
source "${LIB_DIR}/utils/tui.sh"

# All available theme IDs (order must match TUI menu options)
readonly _THEME_IDS=("catppuccin-mocha" "tokyo-night" "dracula" "nord" "gruvbox" "rose-pine" "everforest" "onedark")

# Shared theme preview callback for TUI
_theme_preview_callback() {
    local idx="$1"
    _tui_render_theme_preview "${_THEME_IDS[$idx]}"
    # _tui_render_theme_preview outputs 4 lines: \n + "Preview: X" + status bar + \n
    _TUI_CALLBACK_LINES=4
}
