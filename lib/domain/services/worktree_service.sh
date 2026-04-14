#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Worktree Service
# Git Worktree management service (Domain layer)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent double-loading
[[ -n "${_WORKTREE_SERVICE_SH_LOADED:-}" ]] && return 0
readonly _WORKTREE_SERVICE_SH_LOADED=1

source "${LIB_DIR}/utils/common.sh"
source "${LIB_DIR}/infra/cache/manager.sh"
source "${LIB_DIR}/infra/config/loader.sh"
source "${LIB_DIR}/utils/i18n.sh"

# Check if inside a Git repository
worktree_check_repo() {
    if ! git_is_inside_work_tree; then
        log_error "$(_i18n ERR_NOT_GIT_REPO)"
        return 1
    fi
    return 0
}

# Get the worktree base path (always returns an absolute path)
# Security: validates resolved path stays within git root
_WORKTREE_BASE_PATH_CACHED=""

worktree_get_base_path() {
    if [[ -n "$_WORKTREE_BASE_PATH_CACHED" ]]; then
        echo "$_WORKTREE_BASE_PATH_CACHED"
        return 0
    fi

    local base_path
    base_path=$(config_get "worktree.base_path")

    if _is_null_or_empty "$base_path"; then
        base_path=".workhive/worktrees"
    fi

    local git_root
    git_root=$(_git_root)

    # Relative paths are always resolved relative to git root
    if [[ "$base_path" != /* ]]; then
        base_path="${git_root}/${base_path}"
    fi

    # Security: resolve and verify path stays within git root
    local resolved
    resolved=$(resolve_realpath_allow_missing "$base_path")

    if [[ "$resolved" != "$git_root"* ]]; then
        log_error "$(_i18n_fmt ERR_WT_BASE_PATH_OUTSIDE_FMT "$resolved")"
        log_error "$(_i18n_fmt ERR_WT_PROJECT_ROOT_FMT "$git_root")"
        echo "${git_root}/.workhive/worktrees"
        return 1
    fi

    _WORKTREE_BASE_PATH_CACHED="$resolved"
    echo "$resolved"
}

# Get the worktree path for a given branch
# Args: $1 = branch name
worktree_get_path() {
    local branch="$1"
    local base_path
    base_path=$(worktree_get_base_path)

    local safe_branch
    safe_branch=$(_sanitize_branch_name "$branch")
    local target_path="${base_path}/${safe_branch}"

    # Convert to absolute path
    if [[ ! "$target_path" = /* ]]; then
        local project_root
        project_root=$(_git_root)
        target_path="${project_root}/${target_path}"
    fi

    # Resolve symlinks for existing paths; for new paths resolve parent to ensure consistency
    if command -v realpath &>/dev/null; then
        if [[ -e "$target_path" ]]; then
            target_path=$(realpath "$target_path")
        else
            local _parent
            _parent=$(dirname "$target_path")
            if [[ -d "$_parent" ]]; then
                target_path="$(realpath "$_parent")/$(basename "$target_path")"
            fi
        fi
    fi

    echo "$target_path"
}

# Check if a worktree exists
# Args: $1 = branch name
# Returns: 0 = exists, 1 = does not exist
worktree_exists() {
    local branch="$1"
    local worktree_path
    worktree_path=$(worktree_get_path "$branch")

    [[ -d "$worktree_path" ]]
}

# List all worktrees
# Returns: newline-separated list of worktree paths
worktree_list_all() {
    if ! worktree_check_repo; then
        return 1
    fi

    local cache_key="worktree:list"
    local cached
    cached=$(cache_get "$cache_key" 2>/dev/null) && {
        echo "$cached"
        return 0
    }

    local worktrees
    worktrees=$(git worktree list --porcelain | grep "^worktree" | cut -d' ' -f2)

    cache_set "$cache_key" "$worktrees" "$CACHE_TTL_WORKTREE_LIST"

    echo "$worktrees"
}

# Create a worktree
# Args: $1 = branch name
# Returns: 0 = success, 1 = failure
_worktree_validate_branch_name() {
    local branch="$1"
    if [[ -z "$branch" ]]; then
        log_error "$(_i18n ERR_BRANCH_EMPTY)"
        return 1
    fi
    if [[ "$branch" =~ [[:space:]] ]]; then
        log_error "$(_i18n ERR_BRANCH_SPACES) $branch"
        return 1
    fi
    if [[ "$branch" == *".."* ]]; then
        log_error "$(_i18n ERR_BRANCH_ILLEGAL_CHARS) $branch"
        return 1
    fi
    # Canonical Git ref validation (authoritative).
    if ! git check-ref-format --branch "$branch" &>/dev/null; then
        log_error "$(_i18n ERR_BRANCH_INVALID) $branch"
        return 1
    fi
    return 0
}

_worktree_add_or_create() {
    local branch="$1"
    local worktree_path="$2"

    if git rev-parse --verify "$branch" &>/dev/null; then
        git worktree add "$worktree_path" "$branch" 2>/dev/null
    else
        log_info "$(_i18n INFO_BRANCH_NOT_EXISTS_CREATING) $branch"
        git worktree add -b "$branch" "$worktree_path" 2>/dev/null
    fi
}

worktree_create() {
    local branch="$1"

    if ! worktree_check_repo; then
        return 1
    fi

    _worktree_validate_branch_name "$branch" || return 1

    local worktree_path
    worktree_path=$(worktree_get_path "$branch")

    # If the worktree already exists
    if worktree_exists "$branch"; then
        log_info "$(_i18n INFO_WT_EXISTS) $worktree_path"
        return 0
    fi
    op_begin "worktree_create" "$branch" "path=$worktree_path"

    # Ensure the base directory exists
    local base_path
    base_path=$(worktree_get_base_path)
    ensure_dir "$base_path"

    log_info "$(_i18n INFO_WT_CREATING) $branch → $worktree_path"

    # Check if the branch is already occupied by an external worktree
    local existing_wt_path
    existing_wt_path=$(git worktree list --porcelain 2>/dev/null | awk -v b="$branch" '
        /^worktree / { wt=$2 }
        /^branch refs\/heads\// {
            sub(/^branch refs\/heads\//, "")
            if ($0 == b) { print wt; exit }
        }
    ')
    if [[ -n "$existing_wt_path" && "$existing_wt_path" != "$worktree_path" ]]; then
        printf '%s\n' "$(_i18n_fmt ERR_WT_EXTERNAL_CONFLICT "$branch" "$existing_wt_path")" >&2
        echo "" >&2
        log_info "$(_i18n ERR_WT_EXTERNAL_HINT)"
        echo "  git worktree remove \"$existing_wt_path\"" >&2
        echo "" >&2
        log_info "$(_i18n ERR_WT_EXTERNAL_OR_ADOPT)"
        echo "  git worktree move \"$existing_wt_path\" \"$worktree_path\"" >&2
        op_fail "worktree_create" "$branch" 1 "external_conflict=$existing_wt_path"
        return 1
    fi

    # Attempt worktree creation; on failure, prune stale records and retry once.
    if ! _worktree_add_or_create "$branch" "$worktree_path"; then
        log_debug "worktree add failed, pruning stale records and retrying"
        git worktree prune 2>/dev/null || true
        if ! _worktree_add_or_create "$branch" "$worktree_path"; then
            log_error "$(_i18n ERR_WT_CREATE_FAILED)"
            op_fail "worktree_create" "$branch" 1 "git_worktree_add_failed_after_prune"
            return 1
        fi
    fi

    # Invalidate cache
    cache_invalidate "worktree:*"

    log_success "$(_i18n INFO_WT_CREATED) $worktree_path"
    op_ok "worktree_create" "$branch" "path=$worktree_path"
    return 0
}

# Sync files to a newly created worktree
# Copy or symlink specified files from the project root to the worktree directory
# Config format (config.yaml):
#   worktree:
#     sync_files:
#       - source: ".env"
#         mode: "copy"       # copy | symlink
#       - source: "node_modules"
#         mode: "symlink"
#
# Args: $1 = worktree path
# Returns: 0 = success (even if some files are skipped), 1 = fatal error
worktree_sync_files() {
    local worktree_path="$1"

    if [[ ! -d "$worktree_path" ]]; then
        log_error "$(_i18n ERR_WT_NOT_FOUND) $worktree_path"
        return 1
    fi

    local project_root
    project_root=$(_git_root)

    local count=0
    local _sync_config_file=""

    # Direct yq usage: config_get doesn't support YAML array iteration
    if _sync_config_file=$(config_find_file_with_key "worktree.sync_files" 2>/dev/null); then
        count=$(yq '.worktree.sync_files | length' "$_sync_config_file" 2>/dev/null || echo "0")
        [[ "$count" == "null" ]] && count=0
    fi

    if [[ "$count" -eq 0 ]]; then
        log_debug "worktree_sync_files: no sync_files configured, skipping"
        return 0
    fi

    log_info "$(_i18n SYNC_FILES_START) ($count)"
    op_begin "worktree_sync_files" "$worktree_path" "count=$count"

    local synced=0
    local skipped=0
    local failed=0

    local i
    for ((i = 0; i < count; i++)); do
        local source_path=""
        local mode="copy"
        local item_type

        item_type=$(yq ".worktree.sync_files[$i] | type" "$_sync_config_file" 2>/dev/null || echo "!!null")
        if [[ "$item_type" == "!!str" ]]; then
            source_path=$(yq ".worktree.sync_files[$i]" "$_sync_config_file" 2>/dev/null)
            mode="copy"
        else
            source_path=$(yq ".worktree.sync_files[$i].source" "$_sync_config_file" 2>/dev/null)
            mode=$(yq ".worktree.sync_files[$i].mode" "$_sync_config_file" 2>/dev/null)
        fi

        _is_null_or_empty "$source_path" && continue
        _is_null_or_empty "$mode" && mode="copy"

        # Security check: reject path traversal
        if [[ "$source_path" == *".."* ]]; then
            log_warn "$(_i18n SYNC_FILES_PATH_TRAVERSAL) $source_path"
            failed=$((failed + 1))
            continue
        fi

        local abs_source="${project_root}/${source_path}"
        local abs_target="${worktree_path}/${source_path}"

        # Skip if source file/directory does not exist
        if [[ ! -e "$abs_source" ]]; then
            log_debug "sync_files: source not found, skipping: $source_path"
            skipped=$((skipped + 1))
            continue
        fi

        # Skip if target already exists
        if [[ -e "$abs_target" || -L "$abs_target" ]]; then
            log_debug "sync_files: target already exists, skipping: $source_path"
            skipped=$((skipped + 1))
            continue
        fi

        # Ensure the target's parent directory exists
        local target_parent
        target_parent=$(dirname "$abs_target")
        [[ ! -d "$target_parent" ]] && mkdir -p "$target_parent"

        case "$mode" in
            symlink)
                op_begin "worktree_sync_item_symlink" "$source_path" "target=$abs_target"
                if ln -s "$abs_source" "$abs_target" 2>/dev/null; then
                    log_success "  ↗ $source_path (symlink)"
                    op_ok "worktree_sync_item_symlink" "$source_path" "target=$abs_target"
                    synced=$((synced + 1))
                else
                    log_warn "$(_i18n SYNC_FILES_FAILED) $source_path (symlink)"
                    op_fail "worktree_sync_item_symlink" "$source_path" 1 "target=$abs_target"
                    failed=$((failed + 1))
                fi
                ;;
            copy|*)
                op_begin "worktree_sync_item_copy" "$source_path" "target=$abs_target"
                if cp -R -- "$abs_source" "$abs_target" 2>/dev/null; then
                    log_success "  ⤓ $source_path (copy)"
                    op_ok "worktree_sync_item_copy" "$source_path" "target=$abs_target"
                    synced=$((synced + 1))
                else
                    log_warn "$(_i18n SYNC_FILES_FAILED) $source_path (copy)"
                    op_fail "worktree_sync_item_copy" "$source_path" 1 "target=$abs_target"
                    failed=$((failed + 1))
                fi
                ;;
        esac
    done

    if [[ $synced -gt 0 || $skipped -gt 0 ]]; then
        log_info "$(_i18n_fmt SYNC_FILES_DONE "$synced" "$skipped" "$failed")"
    fi
    if [[ $failed -gt 0 ]]; then
        op_fail "worktree_sync_files" "$worktree_path" 1 "synced=$synced skipped=$skipped failed=$failed"
    else
        op_ok "worktree_sync_files" "$worktree_path" "synced=$synced skipped=$skipped"
    fi
    return 0
}

# Remove a worktree
# Args: $1 = branch name, $2 = force remove (optional, default false)
# Returns: 0 = success, 1 = failure
worktree_remove() {
    local branch="$1"
    local force="${2:-false}"

    if ! worktree_check_repo; then
        return 1
    fi

    local worktree_path
    worktree_path=$(worktree_get_path "$branch")

    if ! worktree_exists "$branch"; then
        log_warn "$(_i18n ERR_WT_NOT_FOUND) $worktree_path"
        op_fail "worktree_remove" "$branch" 1 "not_found path=$worktree_path"
        return 1
    fi

    log_info "$(_i18n INFO_WT_REMOVING) $worktree_path"
    op_begin "worktree_remove" "$branch" "path=$worktree_path force=$force"

    # Perform removal
    if [[ "$force" == "true" ]]; then
        if ! git worktree remove --force "$worktree_path"; then
            log_error "$(_i18n ERR_WT_FORCE_REMOVE_FAILED)"
            op_fail "worktree_remove" "$branch" 1 "force_remove_failed path=$worktree_path"
            return 1
        fi
    else
        if ! git worktree remove "$worktree_path"; then
            log_error "$(_i18n ERR_WT_REMOVE_UNCOMMITTED)"
            log_info "$(_i18n INFO_USE_FORCE_REMOVE) workhive rm $branch --force"
            op_fail "worktree_remove" "$branch" 1 "remove_failed path=$worktree_path"
            return 1
        fi
    fi

    # Invalidate cache
    cache_invalidate "worktree:*"

    log_success "$(_i18n INFO_WT_REMOVED) $worktree_path"
    op_ok "worktree_remove" "$branch" "path=$worktree_path force=$force"
    return 0
}

# Prune stale worktree records
worktree_prune() {
    if ! worktree_check_repo; then
        return 1
    fi

    log_info "$(_i18n INFO_WT_PRUNING)"
    op_begin "worktree_prune" "$(_git_root)"
    if ! git worktree prune; then
        op_fail "worktree_prune" "$(_git_root)" 1
        return 1
    fi

    # Invalidate cache
    cache_invalidate "worktree:*"

    log_success "$(_i18n INFO_WT_PRUNED)"
    op_ok "worktree_prune" "$(_git_root)"
}

# Get the branch name of a worktree
# Args: $1 = worktree path
worktree_get_branch() {
    local worktree_path="$1"

    if [[ ! -d "$worktree_path" ]]; then
        return 1
    fi

    # Execute in a subshell to avoid polluting the parent shell's CWD
    (cd "$worktree_path" && git rev-parse --abbrev-ref HEAD 2>/dev/null)
}

# Check whether a path is a managed worktree path under worktree.base_path.
# Args: $1 = absolute or relative path
# Returns: 0 = managed worktree path, 1 = otherwise
worktree_is_managed_path() {
    local wt_path="$1"
    [[ -n "$wt_path" ]] || return 1

    local base_path
    base_path=$(worktree_get_base_path 2>/dev/null) || return 1
    [[ -n "$base_path" ]] || return 1

    # Normalize possible trailing slash in base path for stable prefix check.
    base_path="${base_path%/}"
    [[ "$wt_path" == "$base_path/"* ]]
}

# Validate worktree health
# Args: $1 = branch name
# Returns: 0 = healthy, 1 = has issues
worktree_validate() {
    local branch="$1"
    local worktree_path
    worktree_path=$(worktree_get_path "$branch")

    if ! worktree_exists "$branch"; then
        log_error "$(_i18n ERR_WT_NOT_FOUND) $worktree_path"
        return 1
    fi

    # Check if .git file exists
    if [[ ! -f "$worktree_path/.git" ]]; then
        log_error "$(_i18n ERR_WT_CORRUPTED_GIT)"
        return 1
    fi

    # Check if branch name can be retrieved
    local current_branch
    current_branch=$(worktree_get_branch "$worktree_path")

    if [[ -z "$current_branch" ]]; then
        log_error "$(_i18n ERR_WT_CORRUPTED_BRANCH)"
        return 1
    fi

    log_debug "$(_i18n DEBUG_WORKTREE_HEALTHY) $worktree_path ($current_branch)"
    return 0
}
