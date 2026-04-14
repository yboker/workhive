#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Layout Visual - Shared layout visualization helpers
# Used by config_command and list_command (App shared layer)
#
# Unified Column-based DSL — dynamic ASCII visualization
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[[ -n "${_LAYOUT_VISUAL_SH_LOADED:-}" ]] && return 0
readonly _LAYOUT_VISUAL_SH_LOADED=1

# Dependencies (expected to be already loaded by caller):
#   lib/utils/i18n.sh         — _i18n, _wpad
#   lib/utils/common.sh       — _wpad, _is_null_or_empty
#   lib/infra/config/loader.sh
#   lib/app/shared/command_builder.sh — _cmd_display_label

# ──────────────────────────────────────────────────
# Print layout as a visual ASCII table with commands
# Always renders Column-based DSL from config
# $1 = mode (ignored, kept for API compat), $2 = scope
# ──────────────────────────────────────────────────
_config_print_layout_visual() {
    # shellcheck disable=SC2034
    local mode="$1"
    local scope="${2:-project}"

    _layout_print_columns_visual "$scope"
}

_layout_print_columns_visual() {
    local scope="${1:-project}"
    echo "  $(_i18n LAYOUT_COLUMNS_DESC)"
    echo ""

    _layout_normalize 2>/dev/null || return

    # shellcheck disable=SC2034
    local c p idx=0
    local col_width=22

    local top="  ┌"
    for ((c=0; c<_LCOL_COUNT; c++)); do
        ((c > 0)) && top+="┬"
        top+=$(printf '%0.s─' $(seq 1 $col_width))
    done
    top+="┐"
    echo "$top"

    local max_panes=0
    for ((c=0; c<_LCOL_COUNT; c++)); do
        (( ${_LCOL_PANE_COUNTS[$c]} > max_panes )) && max_panes=${_LCOL_PANE_COUNTS[$c]}
    done

    local row
    for ((row=0; row<max_panes; row++)); do
        if ((row > 0)); then
            local sep="  ├"
            for ((c=0; c<_LCOL_COUNT; c++)); do
                ((c > 0)) && sep+="┼"
                if ((row < ${_LCOL_PANE_COUNTS[$c]})); then
                    sep+=$(printf '%0.s─' $(seq 1 $col_width))
                else
                    sep+=$(printf '%0.s ' $(seq 1 $col_width))
                fi
            done
            sep+="┤"
            echo "$sep"
        fi

        local line="  │"
        for ((c=0; c<_LCOL_COUNT; c++)); do
            local pc=${_LCOL_PANE_COUNTS[$c]}
            if ((row < pc)); then
                local gi=0 cc
                for ((cc=0; cc<c; cc++)); do
                    gi=$((gi + ${_LCOL_PANE_COUNTS[$cc]}))
                done
                gi=$((gi + row))
                local role="${_LPANE_ROLES[$gi]:-}"
                local label
                if [[ -n "$role" ]] && ! _is_null_or_empty "$role"; then
                    label="$role"
                else
                    label="C$((c+1))P$((row+1))"
                fi
                label="$label (${_LCOL_WIDTHS[$c]}%)"
                line+="  $(_wpad $((col_width - 2)) "$label")"
            else
                line+=$(printf '%0.s ' $(seq 1 $col_width))
            fi
            line+="│"
        done
        echo "$line"

        line="  │"
        for ((c=0; c<_LCOL_COUNT; c++)); do
            local pc=${_LCOL_PANE_COUNTS[$c]}
            if ((row < pc)); then
                local gi=0 cc
                for ((cc=0; cc<c; cc++)); do
                    gi=$((gi + ${_LCOL_PANE_COUNTS[$cc]}))
                done
                gi=$((gi + row))
                local cmd="${_LPANE_COMMANDS[$gi]}"
                local cmd_label
                cmd_label=$(_cmd_display_label "$cmd")
                line+="  $(_wpad $((col_width - 2)) "$cmd_label")"
            else
                line+=$(printf '%0.s ' $(seq 1 $col_width))
            fi
            line+="│"
        done
        echo "$line"
    done

    local bottom="  └"
    for ((c=0; c<_LCOL_COUNT; c++)); do
        ((c > 0)) && bottom+="┴"
        bottom+=$(printf '%0.s─' $(seq 1 $col_width))
    done
    bottom+="┘"
    echo "$bottom"
}

# ──────────────────────────────────────────────────
# Preview columns layout from in-memory values.
# $1 col_count, $2 widths array name, $3 pane_counts array name, $4 pane_cmds array name
# ──────────────────────────────────────────────────
layout_preview_columns_runtime() {
    local col_count="$1"
    local widths_name="$2"
    local pane_counts_name="$3"
    local pane_cmds_name="$4"

    # shellcheck disable=SC2178,SC2034
    local -n _pv_widths_ref="$widths_name"
    # shellcheck disable=SC2178,SC2034
    local -n _pv_pane_counts_ref="$pane_counts_name"
    # shellcheck disable=SC2178,SC2034
    local -n _pv_pane_cmds_ref="$pane_cmds_name"

    local top="     ┌"
    local c row
    local -a col_widths=()
    local max_panes=0

    for ((c=0; c<col_count; c++)); do
        local pane_count="${_pv_pane_counts_ref[$c]:-1}"
        (( pane_count > max_panes )) && max_panes=$pane_count
    done

    if (( max_panes < 1 )); then
        max_panes=1
    fi

    for ((c=0; c<col_count; c++)); do
        local pane_count="${_pv_pane_counts_ref[$c]:-1}"
        local w=0
        for ((row=0; row<pane_count; row++)); do
            local gi=0 cc
            for ((cc=0; cc<c; cc++)); do
                gi=$((gi + ${_pv_pane_counts_ref[$cc]:-0}))
            done
            gi=$((gi + row))
            local pane_cmd="${_pv_pane_cmds_ref[$gi]:-null}"
            local pane_label
            pane_label=$(_cmd_display_label "$pane_cmd")
            local pane_width
            pane_width=$(_dw "$pane_label")
            (( pane_width > w )) && w=$pane_width
        done
        local width_label="${_pv_widths_ref[$c]:-0}%"
        local width_label_len
        width_label_len=$(_dw "($width_label)")
        (( width_label_len > w )) && w=$width_label_len
        (( w < 9 )) && w=9
        col_widths+=("$w")
        (( c > 0 )) && top+="┬"
        top+=$(printf '%0.s─' $(seq 1 $((w + 2))))
    done
    top+="┐"
    echo "$top"

    for ((row=0; row<max_panes; row++)); do
        if ((row > 0)); then
            local sep="     "
            if (( row < ${_pv_pane_counts_ref[0]:-0} )); then
                sep+="├"
            else
                sep+="│"
            fi
            for ((c=0; c<col_count; c++)); do
                if (( row < ${_pv_pane_counts_ref[$c]:-0} )); then
                    sep+=$(printf '%0.s─' $(seq 1 $(( ${col_widths[$c]} + 2 ))))
                else
                    sep+=$(printf '%0.s ' $(seq 1 $(( ${col_widths[$c]} + 2 ))))
                fi
                if (( c < col_count - 1 )); then
                    local left_has=0
                    local right_has=0
                    (( row < ${_pv_pane_counts_ref[$c]:-0} )) && left_has=1
                    (( row < ${_pv_pane_counts_ref[$((c+1))]:-0} )) && right_has=1
                    if (( left_has == 1 && right_has == 1 )); then
                        sep+="┼"
                    elif (( left_has == 1 && right_has == 0 )); then
                        sep+="┤"
                    elif (( left_has == 0 && right_has == 1 )); then
                        sep+="├"
                    else
                        sep+="│"
                    fi
                fi
            done
            if (( row < ${_pv_pane_counts_ref[$((col_count-1))]:-0} )); then
                sep+="┤"
            else
                sep+="│"
            fi
            echo "$sep"
        fi

        local line_cmd="     │"
        local line_meta="     │"
        for ((c=0; c<col_count; c++)); do
            local pane_count="${_pv_pane_counts_ref[$c]:-0}"
            local col_w="${col_widths[$c]}"
            if (( row < pane_count )); then
                local gi=0 cc
                for ((cc=0; cc<c; cc++)); do
                    gi=$((gi + ${_pv_pane_counts_ref[$cc]:-0}))
                done
                gi=$((gi + row))
                local pane_cmd="${_pv_pane_cmds_ref[$gi]:-null}"
                local pane_label
                pane_label=$(_cmd_display_label "$pane_cmd")
                line_cmd+=" $(_wpad "$col_w" "$pane_label") │"

                local width_label="(${_pv_widths_ref[$c]:-0}%, 50%)"
                if (( pane_count == 1 )); then
                    width_label="(${_pv_widths_ref[$c]:-0}%)"
                fi
                line_meta+=" $(_wpad "$col_w" "$width_label") │"
            else
                line_cmd+=" $(_wpad "$col_w" "") │"
                line_meta+=" $(_wpad "$col_w" "") │"
            fi
        done
        echo "$line_cmd"
        echo "$line_meta"
    done

    local bottom="     └"
    for ((c=0; c<col_count; c++)); do
        ((c > 0)) && bottom+="┴"
        bottom+=$(printf '%0.s─' $(seq 1 $(( ${col_widths[$c]} + 2 ))))
    done
    bottom+="┘"
    echo "$bottom"

    echo ""
    local pane_idx=0
    local pi
    for ((c=0; c<col_count; c++)); do
        local pane_count="${_pv_pane_counts_ref[$c]:-0}"
        for ((pi=0; pi<pane_count; pi++)); do
            local pane_label
            pane_label=$(_i18n_fmt COLUMNS_PANE_LABEL "$((c+1))" "$((pi+1))")
            local cmd="${_pv_pane_cmds_ref[$pane_idx]:-null}"
            _is_null_or_empty "$cmd" && cmd="$(_i18n EMPTY_SHELL)"
            echo "  $pane_label: $cmd"
            pane_idx=$((pane_idx + 1))
        done
    done
    echo ""
}

# ──────────────────────────────────────────────────
# Determine config source for layout
# Returns: "project" | "global" | "default"
# ──────────────────────────────────────────────────
_layout_config_source() {
    local cfg_file
    cfg_file=$(config_find_file_with_key "layout.columns" 2>/dev/null)
    if [[ -n "$cfg_file" ]]; then
        local project_cfg
        project_cfg="$(_workhive_dir)/config.yaml"
        if [[ "$cfg_file" == "$project_cfg" ]]; then
            echo "project"
        else
            echo "global"
        fi
    else
        echo "default"
    fi
}

_layout_config_source_label() {
    local source="$1"
    case "$source" in
        project)
            echo "$(_i18n STATUS_LAYOUT_SRC_PROJECT) ($(_workhive_dir)/config.yaml)"
            ;;
        global)
            echo "$(_i18n STATUS_LAYOUT_SRC_GLOBAL) ($(config_get_global_file))"
            ;;
        default)
            echo "$(_i18n STATUS_LAYOUT_SRC_DEFAULT)"
            ;;
    esac
}
