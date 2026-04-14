#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Dependency Manager
# Extensible framework for managing external dependencies.
# Each dependency is a "provider" script under deps/providers/.
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[[ -n "${_DEPS_MANAGER_SH_LOADED:-}" ]] && return 0
readonly _DEPS_MANAGER_SH_LOADED=1

source "${LIB_DIR}/utils/common.sh"
[[ -n "${LIB_DIR:-}" ]] && source "${LIB_DIR}/utils/i18n.sh" 2>/dev/null || true

readonly _DEPS_PROVIDERS_DIR="${LIB_DIR}/infra/deps/providers"

# Registry: dep_id -> provider script path
declare -gA _DEPS_REGISTRY=()
_DEPS_REGISTRY_LOADED=false

# ──────────────────────────────────────────────────
# Provider contract (each provider script must implement):
#   dep_<id>_name()      → human-readable name
#   dep_<id>_desc()      → short description
#   dep_<id>_scope()     → "global" or "project" (default: "project")
#   dep_<id>_config_key()→ config.yaml key that controls enable/disable
#   dep_<id>_is_enabled()→ 0 if enabled in config, 1 if disabled
#   dep_<id>_is_installed() → 0 if installed, 1 if not
#   dep_<id>_install()   → install the dependency
#   dep_<id>_uninstall() → uninstall/deactivate the dependency
#   dep_<id>_upgrade()   → upgrade the dependency to latest version
#   dep_<id>_status()    → print status info to stdout
#   dep_<id>_doctor()    → print health check info, return 0 if healthy
# ──────────────────────────────────────────────────

_deps_load_providers() {
    [[ "$_DEPS_REGISTRY_LOADED" == true ]] && return 0

    if [[ ! -d "$_DEPS_PROVIDERS_DIR" ]]; then
        _DEPS_REGISTRY_LOADED=true
        return 0
    fi

    local provider_file dep_id
    for provider_file in "$_DEPS_PROVIDERS_DIR"/*.sh; do
        [[ -f "$provider_file" ]] || continue
        dep_id="${provider_file##*/}"; dep_id="${dep_id%.sh}"
        # shellcheck source=/dev/null
        source "$provider_file"
        _DEPS_REGISTRY["$dep_id"]="$provider_file"
        log_debug "Registered dependency provider: $dep_id"
    done

    _DEPS_REGISTRY_LOADED=true
}

deps_list_ids() {
    _deps_load_providers
    local dep_id
    for dep_id in "${!_DEPS_REGISTRY[@]}"; do
        declare -f "dep_${dep_id}_name" &>/dev/null || continue
        echo "$dep_id"
    done | sort
}

deps_get_name() {
    local dep_id="$1"
    _deps_load_providers
    if declare -f "dep_${dep_id}_name" &>/dev/null; then
        "dep_${dep_id}_name"
    else
        echo "$dep_id"
    fi
}

deps_get_desc() {
    local dep_id="$1"
    _deps_load_providers
    if declare -f "dep_${dep_id}_desc" &>/dev/null; then
        "dep_${dep_id}_desc"
    else
        echo ""
    fi
}

deps_get_scope() {
    local dep_id="$1"
    _deps_load_providers
    if declare -f "dep_${dep_id}_scope" &>/dev/null; then
        "dep_${dep_id}_scope"
    else
        echo "project"
    fi
}

deps_is_enabled() {
    local dep_id="$1"
    _deps_load_providers
    if declare -f "dep_${dep_id}_is_enabled" &>/dev/null; then
        "dep_${dep_id}_is_enabled"
    else
        return 1
    fi
}

deps_is_installed() {
    local dep_id="$1"
    _deps_load_providers
    if declare -f "dep_${dep_id}_is_installed" &>/dev/null; then
        "dep_${dep_id}_is_installed"
    else
        return 1
    fi
}

deps_install() {
    local dep_id="$1"
    _deps_load_providers
    if ! declare -f "dep_${dep_id}_install" &>/dev/null; then
        log_error "$(_i18n_fmt DEPS_ERR_UNKNOWN_DEP_FMT "$dep_id")"
        return 1
    fi
    "dep_${dep_id}_install"
}

deps_uninstall() {
    local dep_id="$1"
    _deps_load_providers
    if ! declare -f "dep_${dep_id}_uninstall" &>/dev/null; then
        log_error "$(_i18n_fmt DEPS_ERR_UNKNOWN_DEP_FMT "$dep_id")"
        return 1
    fi
    "dep_${dep_id}_uninstall"
}

deps_status() {
    local dep_id="$1"
    _deps_load_providers
    if declare -f "dep_${dep_id}_status" &>/dev/null; then
        "dep_${dep_id}_status"
    fi
}

deps_sync() {
    local dep_id="$1"
    _deps_load_providers
    if declare -f "dep_${dep_id}_sync" &>/dev/null; then
        "dep_${dep_id}_sync"
    fi
}

deps_doctor() {
    local dep_id="$1"
    _deps_load_providers
    if declare -f "dep_${dep_id}_doctor" &>/dev/null; then
        "dep_${dep_id}_doctor"
    else
        return 0
    fi
}

deps_upgrade() {
    local dep_id="$1"
    _deps_load_providers
    if ! declare -f "dep_${dep_id}_upgrade" &>/dev/null; then
        log_warn "$(_i18n_fmt DEPS_WARN_UPGRADE_UNSUPPORTED_FMT "$dep_id")"
        return 1
    fi
    "dep_${dep_id}_upgrade"
}

deps_upgrade_all() {
    _deps_load_providers
    local dep_id
    local _ua_upgraded=0
    local _ua_failed=0
    local _ua_skipped=0
    for dep_id in $(deps_list_ids); do
        local dep_name
        dep_name=$(deps_get_name "$dep_id")

        if ! deps_is_installed "$dep_id"; then
            log_info "  ⊘ $dep_name: $(_i18n DEPS_UPGRADE_SKIP_NOT_INSTALLED)"
            _ua_skipped=$((_ua_skipped + 1))
            continue
        fi

        if ! declare -f "dep_${dep_id}_upgrade" &>/dev/null; then
            log_info "  ⊘ $dep_name: $(_i18n DEPS_UPGRADE_NOT_SUPPORTED)"
            _ua_skipped=$((_ua_skipped + 1))
            continue
        fi

        log_info "  ⟳ $dep_name $(_i18n DEPS_UPGRADING_ELLIPSIS)"
        if deps_upgrade "$dep_id"; then
            _ua_upgraded=$((_ua_upgraded + 1))
        else
            log_warn "  ✗ $dep_name: $(_i18n DEPS_UPGRADE_FAILED)"
            _ua_failed=$((_ua_failed + 1))
        fi
    done

    # shellcheck disable=SC2034
    DEPS_UPGRADE_RESULT_UPGRADED=$_ua_upgraded
    # shellcheck disable=SC2034
    DEPS_UPGRADE_RESULT_FAILED=$_ua_failed
    # shellcheck disable=SC2034
    DEPS_UPGRADE_RESULT_SKIPPED=$_ua_skipped
}

# Reconcile all dependencies: install enabled ones, uninstall disabled ones.
# Called during init, config set, or open.
deps_reconcile() {
    _deps_load_providers
    local dep_id
    for dep_id in $(deps_list_ids); do
        local dep_name
        dep_name=$(deps_get_name "$dep_id")
        if deps_is_enabled "$dep_id"; then
            if deps_is_installed "$dep_id"; then
                log_debug "Dependency loaded: $dep_name [$(deps_get_scope "$dep_id")]"
                deps_sync "$dep_id"
            else
                log_info "$(_i18n DEPS_INSTALLING): $dep_name..."
                if deps_install "$dep_id"; then
                    log_success "$(_i18n DEPS_INSTALLED): $dep_name"
                else
                    log_warn "$(_i18n DEPS_INSTALL_FAILED): $dep_name"
                fi
            fi
        else
            if deps_is_installed "$dep_id"; then
                log_info "$(_i18n DEPS_REMOVING): $dep_name..."
                if deps_uninstall "$dep_id"; then
                    log_success "$(_i18n DEPS_REMOVED): $dep_name"
                else
                    log_warn "$(_i18n DEPS_REMOVE_FAILED): $dep_name"
                fi
            fi
        fi
    done
}

# Print a summary table of all dependencies for status/doctor commands.
deps_status_summary() {
    _deps_load_providers
    local dep_id
    local has_deps=false

    for dep_id in $(deps_list_ids); do
        has_deps=true
        local name enabled_str installed_str
        name=$(deps_get_name "$dep_id")

        if deps_is_enabled "$dep_id"; then
            enabled_str="${GREEN}enabled${NC}"
        else
            enabled_str="${YELLOW}disabled${NC}"
        fi

        if deps_is_installed "$dep_id"; then
            installed_str="${GREEN}✓ installed${NC}"
        else
            installed_str="${RED}✗ not installed${NC}"
        fi

        echo -e "  %-24s %b  %b" "$name" "$enabled_str" "$installed_str"
    done

    if [[ "$has_deps" == false ]]; then
        echo "  (no dependencies registered)"
    fi
}
