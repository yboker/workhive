#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# i18n - Internationalization
# Multi-language support module (Chinese / English)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent double-loading
[[ -n "${_I18N_SH_LOADED:-}" ]] && return 0
readonly _I18N_SH_LOADED=1

# Current language (zh / en)
WORKHIVE_LANG="${WORKHIVE_LANG:-}"

# Config accessor callbacks (set by caller to avoid utils→infra dependency)
# Usage: i18n_set_config_accessor <config_exists_fn> <config_get_fn>
_I18N_CONFIG_EXISTS_FN=""
_I18N_CONFIG_GET_FN=""

i18n_set_config_accessor() {
    _I18N_CONFIG_EXISTS_FN="${1:-}"
    _I18N_CONFIG_GET_FN="${2:-}"
}

# Dispatch bilingual help: call <prefix>_zh or <prefix>_en based on WORKHIVE_LANG
# Auto-loads help_index.sh if the target function is not yet defined.
# Usage: _help_dispatch "_open_command_help"
_help_dispatch() {
    local prefix="$1"
    local suffix="_en"
    [[ "${WORKHIVE_LANG:-en}" == "zh" ]] && suffix="_zh"

    local fn="${prefix}${suffix}"
    if ! type -t "$fn" &>/dev/null; then
        # shellcheck source=/dev/null
        source "${LIB_DIR:-}/app/shared/help_index.sh" 2>/dev/null || true
    fi
    "$fn"
}

# Get translated text (subshell version — use in $() contexts)
# Args: $1 = message key
# Returns: text in the current language via stdout
_i18n() {
    local _K="_I18N_${WORKHIVE_LANG}_${1}"
    local _V="${!_K:-}"
    if [[ -n "$_V" ]]; then
        echo "$_V"
    else
        _K="_I18N_en_${1}"
        echo "${!_K:-$1}"
    fi
}

# Zero-fork i18n: sets global $_v instead of echo (avoids subshell)
# Usage: _i18n_v KEY; echo "$_v"
# Hot-path alternative to $(_i18n KEY) — saves ~2ms per call
_i18n_v() {
    local _K="_I18N_${WORKHIVE_LANG}_${1}"
    _v="${!_K:-}"
    if [[ -z "$_v" ]]; then
        _K="_I18N_en_${1}"
        _v="${!_K:-$1}"
    fi
}

# Formatted i18n helper to reduce repeated printf "$(_i18n KEY)" patterns.
# Usage: _i18n_fmt KEY arg1 arg2 ...
_i18n_fmt() {
    local key="$1"
    shift || true
    printf "$(_i18n "$key")" "$@"
}

# ──────────────────────────────────────────────────
# Language selection (called at the start of init/config)
# ──────────────────────────────────────────────────

# Prompt user to select language (if not already set)
# Args: --force  force re-selection (ignore existing config)
i18n_ask_language() {
    local force_ask=false
    [[ "${1:-}" == "--force" ]] && force_ask=true

    # Force mode: clear existing setting
    if [[ "$force_ask" == true ]]; then
        WORKHIVE_LANG=""
    fi

    if [[ -n "$WORKHIVE_LANG" ]]; then
        return 0
    fi

    # Try reading from config file (non-force mode, via callback to avoid direct infra dependency)
    if [[ "$force_ask" == false && -n "$_I18N_CONFIG_EXISTS_FN" && -n "$_I18N_CONFIG_GET_FN" ]]; then
        if "$_I18N_CONFIG_EXISTS_FN" 2>/dev/null; then
            local saved_lang
            saved_lang=$("$_I18N_CONFIG_GET_FN" "language" 2>/dev/null || echo "")
            if ! _is_null_or_empty "$saved_lang"; then
                WORKHIVE_LANG="$saved_lang"
                return 0
            fi
        fi
    fi

    # Non-TTY environment: default to English
    if ! [ -c /dev/tty ] 2>/dev/null || ! [ -t 0 ]; then
        WORKHIVE_LANG="en"
        return 0
    fi

    # Interactive selection
    source "${LIB_DIR}/utils/tui.sh"
    # shellcheck disable=SC2034
    local options=("English" "Chinese")
    tui_select_menu "Language" options 0 ""

    case "$TUI_SELECTED_INDEX" in
        1) WORKHIVE_LANG="zh" ;;
        *) WORKHIVE_LANG="en" ;;
    esac
}

# ──────────────────────────────────────────────────
# Translation string registry
# Naming convention: _I18N_<lang>_<KEY>
# ──────────────────────────────────────────────────

# === init command ===

_I18N_en_PREVIEW="Preview:"
_I18N_zh_PREVIEW="预览:"

_I18N_en_INIT_WIZARD="workhive init wizard"
_I18N_zh_INIT_WIZARD="workhive 初始化向导"

_I18N_en_CONFIG_EXISTS="Config file already exists: .workhive/config.yaml"
_I18N_zh_CONFIG_EXISTS="配置文件已存在: .workhive/config.yaml"

_I18N_en_OVERWRITE_CONFIG="Overwrite existing config?"
_I18N_zh_OVERWRITE_CONFIG="是否覆盖现有配置?"

_I18N_en_INIT_CANCELLED="Init cancelled"
_I18N_zh_INIT_CANCELLED="已取消初始化"

_I18N_en_USE_CONFIG_CMD="Use 'workhive config' to modify config"
_I18N_zh_USE_CONFIG_CMD="使用 'workhive config' 修改配置"

_I18N_en_USE_FORCE="Use --force to overwrite existing config"
_I18N_zh_USE_FORCE="使用 --force 覆盖现有配置"

_I18N_en_STEP_DEPS="Step 1/4: Checking dependencies..."
_I18N_zh_STEP_DEPS="步骤 1/4: 检测依赖..."

_I18N_en_REQUIRED_DEPS="Required dependencies:"
_I18N_zh_REQUIRED_DEPS="必需依赖:"

_I18N_en_OPTIONAL_TOOLS="Optional tools:"
_I18N_zh_OPTIONAL_TOOLS="可选工具:"

_I18N_en_PANE_TOOLS="Pane tools (launchable in pane):"
_I18N_zh_PANE_TOOLS="Pane 工具 (可在 pane 中启动):"

_I18N_en_DEPS_MISSING="Required dependencies missing, please install first"
_I18N_zh_DEPS_MISSING="必需依赖缺失，请先安装后重试"

_I18N_en_INSTALL_GUIDE="Installation guide:"
_I18N_zh_INSTALL_GUIDE="安装方法:"

_I18N_en_STEP_TEMPLATE="Step 2/4: Choose workflow template"
_I18N_zh_STEP_TEMPLATE="步骤 2/4: 选择工作流模板"

_I18N_en_SELECT_TEMPLATE="Choose workflow template"
_I18N_zh_SELECT_TEMPLATE="选择工作流模板"

_I18N_en_DEFAULT_TEMPLATE="Using default template: AI Coding (left-split)"
_I18N_zh_DEFAULT_TEMPLATE="使用默认模板: AI Coding (left-split)"

_I18N_en_SELECTED="Selected"
_I18N_zh_SELECTED="已选择"

_I18N_en_LAYOUT="Layout"
_I18N_zh_LAYOUT="布局"

_I18N_en_FINE_TUNE="Adjust pane commands?"
_I18N_zh_FINE_TUNE="是否调整 Pane 命令?"

_I18N_en_STEP_THEME="Step 3/4: Configure tmux theme"
_I18N_zh_STEP_THEME="步骤 3/4: 配置 tmux 主题"

_I18N_en_SELECT_THEME="Choose tmux theme"
_I18N_zh_SELECT_THEME="选择 tmux 主题"

_I18N_en_DEFAULT_THEME="Using default theme"
_I18N_zh_DEFAULT_THEME="使用默认主题"

_I18N_en_GENERATING_CONFIG="Generating config file..."
_I18N_zh_GENERATING_CONFIG="生成配置文件..."

_I18N_en_CONFIG_CREATED="Config file created: .workhive/config.yaml"
_I18N_zh_CONFIG_CREATED="配置文件已创建: .workhive/config.yaml"

_I18N_en_INIT_COMPLETE="Init complete!"
_I18N_zh_INIT_COMPLETE="初始化完成!"

_I18N_en_NEXT_STEPS="Next steps:"
_I18N_zh_NEXT_STEPS="下一步操作:"

_I18N_en_NEXT_OPEN="Select or create workspace:  workhive open"
_I18N_zh_NEXT_OPEN="选择或创建工作空间:  workhive open"

_I18N_en_NEXT_WORKTREE="Create new Worktree:  workhive open <worktree>"
_I18N_zh_NEXT_WORKTREE="创建新 Worktree:  workhive open <worktree>"

_I18N_en_NEXT_LIST="List all workspaces:  workhive ls"
_I18N_zh_NEXT_LIST="查看所有空间:    workhive ls"

_I18N_en_NEXT_CONFIG="Modify config:        workhive config"
_I18N_zh_NEXT_CONFIG="修改配置:        workhive config"

# === layout ===

_I18N_en_SELECT_LAYOUT="Choose layout mode"
_I18N_zh_SELECT_LAYOUT="选择布局模式"

_I18N_en_LAYOUT_LEFT="left-split — Left split (60%) + Right main (40%)"
_I18N_zh_LAYOUT_LEFT="left-split — 左侧上下分栏 (60%) + 右侧主 Pane (40%)"

_I18N_en_LAYOUT_RIGHT="right-split — Left main (40%) + Right split (60%)"
_I18N_zh_LAYOUT_RIGHT="right-split — 左侧主 Pane (40%) + 右侧上下分栏 (60%)"

_I18N_en_LAYOUT_QUAD="quad — Four panes (TL + BL + TR + BR)"
_I18N_zh_LAYOUT_QUAD="quad — 四分屏 (左上 + 左下 + 右上 + 右下)"

_I18N_en_LAYOUT_HALF="half-split — Simple 50/50 left-right split"
_I18N_zh_LAYOUT_HALF="half-split — 左右对半分屏 (50% + 50%)"

# === pane names ===

_I18N_en_PANE_TL="Top-Left Pane"
_I18N_zh_PANE_TL="左上 Pane"

_I18N_en_PANE_BL="Bottom-Left Pane"
_I18N_zh_PANE_BL="左下 Pane"

_I18N_en_PANE_R="Right Pane"
_I18N_zh_PANE_R="右侧 Pane"

_I18N_en_PANE_L="Left Pane"
_I18N_zh_PANE_L="左侧 Pane"

_I18N_en_PANE_TR="Top-Right Pane"
_I18N_zh_PANE_TR="右上 Pane"

_I18N_en_PANE_BR="Bottom-Right Pane"
_I18N_zh_PANE_BR="右下 Pane"

_I18N_en_PANE_R_MAIN="Right Pane (main)"
_I18N_zh_PANE_R_MAIN="右侧主 Pane"

_I18N_en_PANE_L_MAIN="Left Pane (main)"
_I18N_zh_PANE_L_MAIN="左侧主 Pane"

# === command select ===

_I18N_en_PANE_CMD_PROMPT="startup command for"
_I18N_zh_PANE_CMD_PROMPT="的启动命令"

_I18N_en_CURRENT="Current"
_I18N_zh_CURRENT="当前"

_I18N_en_EMPTY_SHELL="<Empty Shell>"
_I18N_zh_EMPTY_SHELL="<空 Shell>"

_I18N_en_CUSTOM_INPUT="Custom input..."
_I18N_zh_CUSTOM_INPUT="自定义输入..."

_I18N_en_EMPTY_SHELL_OPT="Empty Shell"
_I18N_zh_EMPTY_SHELL_OPT="空 Shell"

_I18N_en_YAZI_DESC="File Browser"
_I18N_zh_YAZI_DESC="文件浏览器"

_I18N_en_INPUT_CMD="Enter command>"
_I18N_zh_INPUT_CMD="输入命令>"

# === deps ===

_I18N_en_NOT_INSTALLED="not installed"
_I18N_zh_NOT_INSTALLED="未安装"

_I18N_en_LAZYGIT_HINT="lazygit not installed — Git TUI for visual commits/branches (brew install lazygit)"
_I18N_zh_LAZYGIT_HINT="lazygit 未安装 — Git 图形界面,可视化提交/分支管理 (brew install lazygit)"

_I18N_en_CLAUDE_HINT="claude not installed — AI coding assistant for pane integration (npm install -g @anthropic-ai/claude-code)"
_I18N_zh_CLAUDE_HINT="claude 未安装 — AI 编程助手,集成到 pane 中使用 (npm install -g @anthropic-ai/claude-code)"

_I18N_en_CODEX_HINT="codex not installed — OpenAI AI coding agent for pane integration (npm install -g @openai/codex)"
_I18N_zh_CODEX_HINT="codex 未安装 — OpenAI AI 编程代理,集成到 pane 中使用 (npm install -g @openai/codex)"

_I18N_en_OPENCODE_HINT="opencode not installed — AI coding agent for pane integration (npm i -g opencode-ai@latest)"
_I18N_zh_OPENCODE_HINT="opencode 未安装 — AI 编程代理,集成到 pane 中使用 (npm i -g opencode-ai@latest)"

_I18N_en_YAZI_HINT="yazi not installed — terminal file browser for pane integration (brew install yazi)"
_I18N_zh_YAZI_HINT="yazi 未安装 — 终端文件浏览器,集成到 pane 中使用 (brew install yazi)"

# Dependency descriptions (shown in init dependency check)
_I18N_en_DEP_GIT="Version Control"
_I18N_zh_DEP_GIT="版本控制"

_I18N_en_DEP_TMUX="Terminal Multiplexer"
_I18N_zh_DEP_TMUX="终端复用器"

_I18N_en_DEP_YQ="YAML Parser"
_I18N_zh_DEP_YQ="YAML 解析器"

_I18N_en_DEP_LAZYGIT="Git TUI"
_I18N_zh_DEP_LAZYGIT="Git 图形界面"

_I18N_en_DEP_CLAUDE="AI Coding Assistant"
_I18N_zh_DEP_CLAUDE="AI 编程助手"

_I18N_en_DEP_CODEX="AI Coding Agent (OpenAI)"
_I18N_zh_DEP_CODEX="AI 编程代理 (OpenAI)"

_I18N_en_DEP_OPENCODE="AI Coding Agent"
_I18N_zh_DEP_OPENCODE="AI 编程代理"

_I18N_en_DEP_FZF="Fuzzy Finder"
_I18N_zh_DEP_FZF="模糊搜索"

_I18N_en_DEP_DELTA="Git Diff Viewer"
_I18N_zh_DEP_DELTA="Git 差异查看器"

_I18N_en_DEP_STARSHIP="Cross-shell Prompt"
_I18N_zh_DEP_STARSHIP="跨 Shell 提示符"

_I18N_en_DEP_ZOXIDE="Smart Directory Jump"
_I18N_zh_DEP_ZOXIDE="智能目录跳转"

_I18N_en_DEP_ATUIN="Shell History Search"
_I18N_zh_DEP_ATUIN="Shell 历史搜索"

_I18N_en_DEPS_MISSING_LIST="The following required dependencies are not installed"
_I18N_zh_DEPS_MISSING_LIST="以下必需依赖未安装"

# === config command ===

_I18N_en_CONFIG_WIZARD="Configuration wizard"
_I18N_zh_CONFIG_WIZARD="配置修改向导"

_I18N_en_CURRENT_CONFIG="Current config summary"
_I18N_zh_CURRENT_CONFIG="当前配置摘要"

_I18N_en_MODIFY_LAYOUT="Modify layout config?"
_I18N_zh_MODIFY_LAYOUT="是否修改布局配置?"

_I18N_en_MODIFY_LAYOUT_TITLE="Modify layout config"
_I18N_zh_MODIFY_LAYOUT_TITLE="修改布局配置"

_I18N_en_MODIFY_TMUX="Modify tmux config?"
_I18N_zh_MODIFY_TMUX="是否修改 tmux 配置?"

_I18N_en_MODIFY_TMUX_TITLE="Modify tmux config"
_I18N_zh_MODIFY_TMUX_TITLE="修改 tmux 配置"

_I18N_en_MODIFY_TMUX_PERSISTENCE="Modify session persistence?"
_I18N_zh_MODIFY_TMUX_PERSISTENCE="是否修改会话持久化?"

_I18N_en_MODIFY_TMUX_PERSISTENCE_TITLE="Session Persistence"
_I18N_zh_MODIFY_TMUX_PERSISTENCE_TITLE="会话持久化"

_I18N_en_STEP_TMUX_PERSISTENCE="Step 4/4: Session Persistence"
_I18N_zh_STEP_TMUX_PERSISTENCE="步骤 4/4: 会话持久化"

_I18N_en_DESC_TMUX_PERSISTENCE="Auto-save and restore tmux sessions (windows, panes, layout, running commands). Snapshots are stored per-project."
_I18N_zh_DESC_TMUX_PERSISTENCE="自动保存并恢复 tmux 会话（窗口、面板、布局、运行中的命令）。快照按项目存储。"

_I18N_en_ASK_ENABLE_PERSISTENCE="Enable session persistence?"
_I18N_zh_ASK_ENABLE_PERSISTENCE="是否启用会话持久化?"

_I18N_en_DEFAULT_PERSISTENCE="Default persistence"
_I18N_zh_DEFAULT_PERSISTENCE="默认持久化"

_I18N_en_DEPS_TITLE="Dependencies"
_I18N_zh_DEPS_TITLE="依赖管理"

_I18N_en_DEPS_SCOPE_GLOBAL="global"
_I18N_zh_DEPS_SCOPE_GLOBAL="全局"

_I18N_en_DEPS_SCOPE_PROJECT="project"
_I18N_zh_DEPS_SCOPE_PROJECT="项目"

_I18N_en_DEPS_ENABLED="enabled"
_I18N_zh_DEPS_ENABLED="已启用"

_I18N_en_DEPS_DISABLED="disabled"
_I18N_zh_DEPS_DISABLED="已禁用"

_I18N_en_DEPS_NOT_INSTALLED="not installed"
_I18N_zh_DEPS_NOT_INSTALLED="未安装"

_I18N_en_DEPS_LAST_SAVE="last save"
_I18N_zh_DEPS_LAST_SAVE="最后保存"

_I18N_en_DEPS_INSTALLING="Installing dependency"
_I18N_zh_DEPS_INSTALLING="正在安装依赖"

_I18N_en_DEPS_INSTALLED="Dependency installed"
_I18N_zh_DEPS_INSTALLED="依赖已安装"

_I18N_en_DEPS_INSTALL_FAILED="Failed to install dependency"
_I18N_zh_DEPS_INSTALL_FAILED="依赖安装失败"

_I18N_en_DEPS_REMOVING="Removing dependency"
_I18N_zh_DEPS_REMOVING="正在移除依赖"

_I18N_en_DEPS_REMOVED="Dependency removed"
_I18N_zh_DEPS_REMOVED="依赖已移除"

_I18N_en_DEPS_REMOVE_FAILED="Failed to remove dependency"
_I18N_zh_DEPS_REMOVE_FAILED="依赖移除失败"

_I18N_en_DEPS_UPGRADING="Upgrading dependency"
_I18N_zh_DEPS_UPGRADING="正在升级依赖"

_I18N_en_DEPS_UPGRADED="Dependency upgraded"
_I18N_zh_DEPS_UPGRADED="依赖已升级"

_I18N_en_DEPS_UPGRADE_FAILED="Failed to upgrade dependency"
_I18N_zh_DEPS_UPGRADE_FAILED="依赖升级失败"

_I18N_en_DEPS_UPGRADE_SKIP_NOT_INSTALLED="Skipping (not installed)"
_I18N_zh_DEPS_UPGRADE_SKIP_NOT_INSTALLED="跳过 (未安装)"

_I18N_en_DEPS_UPGRADE_NOT_SUPPORTED="Upgrade not supported"
_I18N_zh_DEPS_UPGRADE_NOT_SUPPORTED="不支持升级"

_I18N_en_DEPS_UPGRADE_TITLE="Upgrade Dependencies"
_I18N_zh_DEPS_UPGRADE_TITLE="升级依赖"

_I18N_en_DEPS_UPGRADE_CONFIRM="Upgrade all installed dependencies to latest version?"
_I18N_zh_DEPS_UPGRADE_CONFIRM="将所有已安装的依赖升级到最新版本?"

_I18N_en_DEPS_UPGRADE_NONE_INSTALLED="No dependencies installed. Nothing to upgrade."
_I18N_zh_DEPS_UPGRADE_NONE_INSTALLED="没有已安装的依赖, 无需升级"

_I18N_en_DEPS_UPGRADE_RESULT="Result"
_I18N_zh_DEPS_UPGRADE_RESULT="结果"

_I18N_en_DEPS_UPGRADE_UPGRADED="upgraded"
_I18N_zh_DEPS_UPGRADE_UPGRADED="已升级"

_I18N_en_DEPS_UPGRADE_FAILED_COUNT="failed"
_I18N_zh_DEPS_UPGRADE_FAILED_COUNT="失败"

_I18N_en_DEPS_UPGRADE_SKIPPED="skipped"
_I18N_zh_DEPS_UPGRADE_SKIPPED="跳过"

_I18N_en_DEPS_ALREADY_LATEST="already up to date"
_I18N_zh_DEPS_ALREADY_LATEST="已是最新版本"

_I18N_en_DEPS_UPGRADING_FROM_TO="upgrading"
_I18N_zh_DEPS_UPGRADING_FROM_TO="升级中"

_I18N_en_DEPS_UPGRADING_ELLIPSIS="upgrading..."
_I18N_zh_DEPS_UPGRADING_ELLIPSIS="升级中..."

_I18N_en_DEPS_PERSISTENCE_FIRST_SAVE="Creating initial snapshot..."
_I18N_zh_DEPS_PERSISTENCE_FIRST_SAVE="正在创建初始快照..."

_I18N_en_DEPS_PERSISTENCE_SAVE_OK="Session snapshot saved"
_I18N_zh_DEPS_PERSISTENCE_SAVE_OK="会话快照已保存"

_I18N_en_DEPS_NO_REGISTERED="No dependencies registered"
_I18N_zh_DEPS_NO_REGISTERED="无已注册的依赖"

_I18N_en_DEPS_DETAILED_STATUS="Detailed Status"
_I18N_zh_DEPS_DETAILED_STATUS="详细状态"

_I18N_en_DEPS_STATUS_STATE="Status"
_I18N_zh_DEPS_STATUS_STATE="状态"

_I18N_en_DEPS_STATUS_AUTOSAVE="Auto-save"
_I18N_zh_DEPS_STATUS_AUTOSAVE="自动保存"

_I18N_en_DEPS_STATUS_RESTORE="Auto-restore"
_I18N_zh_DEPS_STATUS_RESTORE="自动恢复"

_I18N_en_DEPS_STATUS_LASTSAVE="Last save"
_I18N_zh_DEPS_STATUS_LASTSAVE="最后保存"

_I18N_en_DEPS_STATUS_NO_SNAPSHOT="no snapshots yet"
_I18N_zh_DEPS_STATUS_NO_SNAPSHOT="尚无快照"

_I18N_en_DEPS_STATUS_SNAPSHOTS="Snapshots"
_I18N_zh_DEPS_STATUS_SNAPSHOTS="快照数量"

_I18N_en_DEPS_STATUS_SAVEDIR="Save dir"
_I18N_zh_DEPS_STATUS_SAVEDIR="保存目录"

_I18N_en_DEPS_STATUS_LASTUPG="Last upgrade"
_I18N_zh_DEPS_STATUS_LASTUPG="最后升级"

_I18N_en_WARN_BRANCH_NAME_SANITIZED="Branch name '%s' contains special characters, will be converted to '%s'"
_I18N_zh_WARN_BRANCH_NAME_SANITIZED="分支名 '%s' 含特殊字符, 将转换为 '%s'"

_I18N_en_ASK_BRANCH_NAME_CONFIRM="Continue with converted name?"
_I18N_zh_ASK_BRANCH_NAME_CONFIRM="使用转换后的名称继续?"

_I18N_en_ERR_WT_EXTERNAL_CONFLICT="Branch '%s' is already checked out in an external worktree at: %s"
_I18N_zh_ERR_WT_EXTERNAL_CONFLICT="分支 '%s' 已被外部 Worktree 占用, 路径: %s"

_I18N_en_ERR_WT_EXTERNAL_HINT="To use this branch with workhive, first remove the external worktree:"
_I18N_zh_ERR_WT_EXTERNAL_HINT="如需在 workhive 中使用此分支, 请先删除外部 Worktree:"

_I18N_en_ERR_WT_EXTERNAL_OR_ADOPT="Or adopt the existing worktree by moving it to the workhive-managed path:"
_I18N_zh_ERR_WT_EXTERNAL_OR_ADOPT="或者将外部 Worktree 迁入 workhive 管理路径:"

_I18N_en_MODIFY_REMOTE="Modify remote config?"
_I18N_zh_MODIFY_REMOTE="是否修改远程连接配置?"

_I18N_en_REMOTE_PLACEHOLDER="Remote config (placeholder)"
_I18N_zh_REMOTE_PLACEHOLDER="远程连接配置 (预留功能)"

_I18N_en_EDIT_FILE="Open config file in editor?"
_I18N_zh_EDIT_FILE="是否需要直接编辑配置文件?"

_I18N_en_CONFIG_MANUAL_EDIT_HINT="To manually edit the config file: %s"
_I18N_zh_CONFIG_MANUAL_EDIT_HINT="如需手动编辑配置文件: %s"
_I18N_en_CONFIG_TOOLS_MANAGE_HINT="Tools are managed separately: workhive tools list|add|edit|remove|set-default|manage"
_I18N_zh_CONFIG_TOOLS_MANAGE_HINT="tools 为独立配置管理: workhive tools list|add|edit|remove|set-default|manage"

_I18N_en_CONFIG_COMPLETE="Config modification complete"
_I18N_zh_CONFIG_COMPLETE="配置修改完成"

_I18N_en_MODIFY_LAYOUT_MODE="Modify layout mode"
_I18N_zh_MODIFY_LAYOUT_MODE="修改布局模式"

# === TUI ===

_I18N_en_TUI_NAV="Use Up/Down to navigate, Enter to confirm"
_I18N_zh_TUI_NAV="使用 ↑/↓ 切换, Enter 确认"

# === open command ===

_I18N_en_ERR_UNKNOWN_OPT="Unknown option:"
_I18N_zh_ERR_UNKNOWN_OPT="未知选项:"

_I18N_en_ERR_NO_POSITIONAL="does not accept positional arguments"
_I18N_zh_ERR_NO_POSITIONAL="命令不接受位置参数"

_I18N_en_ERR_ONE_BRANCH_ONLY="Only one branch name allowed"
_I18N_zh_ERR_ONE_BRANCH_ONLY="只能指定一个分支名"

_I18N_en_MSG_CANCELLED="Cancelled"
_I18N_zh_MSG_CANCELLED="已取消"

_I18N_en_MSG_NO_CMD_CANCELLED="No command selected, cancelled"
_I18N_zh_MSG_NO_CMD_CANCELLED="未选择命令,已取消"

_I18N_en_INFO_MAIN_DIR_MODE="Main directory mode: Using project root"
_I18N_zh_INFO_MAIN_DIR_MODE="主目录模式: 使用项目根目录"

_I18N_en_INFO_OPENING_WT="Opening Worktree:"
_I18N_zh_INFO_OPENING_WT="打开Worktree:"

_I18N_en_STEP_1_4="Step 1/4:"
_I18N_zh_STEP_1_4="步骤 1/4:"

_I18N_en_STEP_1_PREP_DIR="Preparing dev directory..."
_I18N_zh_STEP_1_PREP_DIR="准备开发目录..."

_I18N_en_STEP_2_4="Step 2/4:"
_I18N_zh_STEP_2_4="步骤 2/4:"

_I18N_en_STEP_2_PREP_SESSION="Preparing tmux session..."
_I18N_zh_STEP_2_PREP_SESSION="准备 tmux 会话..."

_I18N_en_STEP_3_4="Step 3/4:"
_I18N_zh_STEP_3_4="步骤 3/4:"

_I18N_en_STEP_3_LAYOUT="Configuring layout"
_I18N_zh_STEP_3_LAYOUT="配置布局"

_I18N_en_STEP_4_4="Step 4/4:"
_I18N_zh_STEP_4_4="步骤 4/4:"

_I18N_en_STEP_4_SKIP_ATTACH="Skip auto-attach (--no-attach)"
_I18N_zh_STEP_4_SKIP_ATTACH="跳过自动附加 (--no-attach)"

_I18N_en_MSG_WORKSPACE_READY="Workspace ready!"
_I18N_zh_MSG_WORKSPACE_READY="工作空间已就绪!"

_I18N_en_MSG_ATTACHING="Attaching to session..."
_I18N_zh_MSG_ATTACHING="正在附加到会话..."

_I18N_en_MSG_ATTACHING_TO="Attaching to session:"
_I18N_zh_MSG_ATTACHING_TO="正在附加到会话:"

_I18N_en_HINT_DETACH="Hint: Use Ctrl+B d to detach"
_I18N_zh_HINT_DETACH="提示: 使用 Ctrl+B d 分离会话"

# === attach command ===

_I18N_en_ERR_MISSING_BRANCH="Missing branch name parameter"
_I18N_zh_ERR_MISSING_BRANCH="缺少分支名参数"

_I18N_en_ERR_SESSION_NOT_FOUND="Session not found:"
_I18N_zh_ERR_SESSION_NOT_FOUND="会话不存在:"

_I18N_en_INFO_ATTACHING="Attaching to session:"
_I18N_zh_INFO_ATTACHING="附加到会话:"

_I18N_en_INFO_READONLY_MODE="Mode: Read-only (view only, no edit)"
_I18N_zh_INFO_READONLY_MODE="模式: 只读 (仅查看,不可编辑)"

# === list command ===

_I18N_en_MSG_LOADING="Loading..."
_I18N_zh_MSG_LOADING="加载中..."

_I18N_en_MSG_LOADING_WORKSPACES="Loading workspaces..."
_I18N_zh_MSG_LOADING_WORKSPACES="加载工作空间..."

_I18N_en_MSG_CHECKING_ENV="Checking environment..."
_I18N_zh_MSG_CHECKING_ENV="检查环境..."

_I18N_en_MSG_NO_ACTIVE_WT="No workspaces found"
_I18N_zh_MSG_NO_ACTIVE_WT="当前没有可显示的工作空间"

_I18N_en_TITLE_ACTIVE_WT="Worktree workspaces"
_I18N_zh_TITLE_ACTIVE_WT="Worktree 工作空间"

_I18N_en_COL_PROJECT="Project"
_I18N_zh_COL_PROJECT="项目"

_I18N_en_COL_BRANCH="Branch"
_I18N_zh_COL_BRANCH="分支"

_I18N_en_COL_WORKTREE="Worktree"
_I18N_zh_COL_WORKTREE="Worktree"

_I18N_en_COL_SESSION="Session"
_I18N_zh_COL_SESSION="会话"

_I18N_en_COL_LAYOUT="Layout"
_I18N_zh_COL_LAYOUT="布局"

_I18N_en_COL_PATH="Path"
_I18N_zh_COL_PATH="路径"

_I18N_en_LEGEND="Legend:"
_I18N_zh_LEGEND="图例:"

_I18N_en_ERR_INVALID_FORMAT="Invalid format:"
_I18N_zh_ERR_INVALID_FORMAT="无效的格式:"

_I18N_en_ERR_SESSIONS_WORKTREES_CONFLICT="--sessions-only and --worktrees-only cannot be used together"
_I18N_zh_ERR_SESSIONS_WORKTREES_CONFLICT="--sessions-only 与 --worktrees-only 不能同时使用"

# === pane command ===

_I18N_en_ERR_MUST_IN_TMUX="This command must be run inside a tmux session"
_I18N_zh_ERR_MUST_IN_TMUX="此命令必须在 tmux 会话内执行"

_I18N_en_ERR_ONE_WORKTREE_ONLY="Only one Worktree name allowed"
_I18N_zh_ERR_ONE_WORKTREE_ONLY="只能指定一个 Worktree 名称"

_I18N_en_TITLE_PANE_CMD="Pane command"
_I18N_zh_TITLE_PANE_CMD="Pane 命令"

_I18N_en_SELECT_WORKTREE="Select Worktree"
_I18N_zh_SELECT_WORKTREE="选择 Worktree"

_I18N_en_SELECT_COMMAND="Select command"
_I18N_zh_SELECT_COMMAND="选择命令"

# === destroy command ===

_I18N_en_TITLE_DESTROY="Destroy workhive completely"
_I18N_zh_TITLE_DESTROY="彻底清除 workhive"

_I18N_en_MSG_RESOURCES_TO_DELETE="The following resources will be permanently deleted:"
_I18N_zh_MSG_RESOURCES_TO_DELETE="以下资源将被永久删除:"

_I18N_en_CONFIRM_DESTROY="Confirm destroy all workhive resources? (irreversible)"
_I18N_zh_CONFIRM_DESTROY="确认彻底清除所有 workhive 资源? (此操作不可逆)"

_I18N_en_CONFIRM_DESTROY_DIRTY="Uncommitted changes will be lost, continue?"
_I18N_zh_CONFIRM_DESTROY_DIRTY="有未提交更改将丢失,确定继续?"

_I18N_en_DESTROY_STEP_1="Step 1/3:"
_I18N_zh_DESTROY_STEP_1="步骤 1/3:"

_I18N_en_DESTROY_STEP_1_DESC="Closing all tmux sessions..."
_I18N_zh_DESTROY_STEP_1_DESC="关闭所有 tmux 会话..."

_I18N_en_DESTROY_STEP_2="Step 2/3:"
_I18N_zh_DESTROY_STEP_2="步骤 2/3:"

_I18N_en_DESTROY_STEP_2_DESC="Deleting all Worktrees..."
_I18N_zh_DESTROY_STEP_2_DESC="删除所有 Worktree..."

_I18N_en_DESTROY_STEP_3="Step 3/3:"
_I18N_zh_DESTROY_STEP_3="步骤 3/3:"

_I18N_en_DESTROY_STEP_3_DESC="Deleting .workhive/ config directory..."
_I18N_zh_DESTROY_STEP_3_DESC="删除 .workhive/ 配置目录..."

_I18N_en_MSG_DESTROY_COMPLETE="Destroy complete!"
_I18N_zh_MSG_DESTROY_COMPLETE="彻底清除完成!"

# === bin/workhive help ===

_I18N_en_HELP_DESC="Cross-platform Worktree Manager"
_I18N_zh_HELP_DESC="跨平台Worktree管理器"

_I18N_en_HELP_USAGE="Usage:"
_I18N_zh_HELP_USAGE="用法:"

_I18N_en_HELP_COMMANDS="Commands:"
_I18N_zh_HELP_COMMANDS="命令:"

_I18N_en_HELP_GLOBAL_OPTIONS="Global options:"
_I18N_zh_HELP_GLOBAL_OPTIONS="全局选项:"

_I18N_en_HELP_CMD_INIT="interactive init"
_I18N_zh_HELP_CMD_INIT="交互式初始化配置"

_I18N_en_HELP_CMD_CONFIG="view/modify config"
_I18N_zh_HELP_CMD_CONFIG="查看/修改配置"

_I18N_en_HELP_CMD_TOOLS="manage global tools registry"
_I18N_zh_HELP_CMD_TOOLS="管理全局工具注册表"

_I18N_en_HELP_CMD_OPEN="create/enter Worktree"
_I18N_zh_HELP_CMD_OPEN="创建/进入Worktree"

_I18N_en_HELP_CMD_ATTACH="attach to existing session"
_I18N_zh_HELP_CMD_ATTACH="附加到现有会话"

_I18N_en_HELP_CMD_LIST="workspace dashboard (all workspaces)"
_I18N_zh_HELP_CMD_LIST="工作空间仪表盘（所有空间）"

_I18N_en_HELP_CMD_DOCTOR="check environment health"
_I18N_zh_HELP_CMD_DOCTOR="检查环境健康状态"

_I18N_en_HELP_CMD_GC="clean up merged branches"
_I18N_zh_HELP_CMD_GC="清理已合并分支"

_I18N_en_HELP_CMD_RESTORE="restore saved sessions after reboot"
_I18N_zh_HELP_CMD_RESTORE="重启后恢复已保存的会话"

_I18N_en_HELP_CMD_LOG="view operation logs"
_I18N_zh_HELP_CMD_LOG="查看操作日志"

_I18N_en_HELP_CMD_REMOVE="destroy Worktree"
_I18N_zh_HELP_CMD_REMOVE="彻底销毁Worktree"

_I18N_en_HELP_CMD_RENAME="rename workspace (branch + worktree + session)"
_I18N_zh_HELP_CMD_RENAME="重命名工作空间 (分支 + worktree + 会话)"

_I18N_en_HELP_CMD_PANE="manage panes (open / close / split / send / list)"
_I18N_zh_HELP_CMD_PANE="管理 pane (开启 / 关闭 / 分割 / 发送 / 列表)"

_I18N_en_HELP_CMD_DESTROY="remove all workhive resources"
_I18N_zh_HELP_CMD_DESTROY="彻底清除所有 workhive 资源"

_I18N_en_HELP_OPT_VERSION="show version"
_I18N_zh_HELP_OPT_VERSION="显示版本信息"

_I18N_en_HELP_OPT_HELP="show help"
_I18N_zh_HELP_OPT_HELP="显示帮助信息"

_I18N_en_HELP_OPT_DEBUG="enable debug mode"
_I18N_zh_HELP_OPT_DEBUG="开启调试模式"

_I18N_en_HELP_SHORTHAND="shorthand:"
_I18N_zh_HELP_SHORTHAND="缩写:"

_I18N_en_HELP_EXAMPLES="Examples:"
_I18N_zh_HELP_EXAMPLES="示例:"

_I18N_en_HELP_EXAMPLE_INIT="Initialize project config"
_I18N_zh_HELP_EXAMPLE_INIT="初始化项目配置"

_I18N_en_HELP_EXAMPLE_OPEN="Create and enter Worktree"
_I18N_zh_HELP_EXAMPLE_OPEN="创建并进入Worktree"

_I18N_en_HELP_EXAMPLE_LS="List all workspaces"
_I18N_zh_HELP_EXAMPLE_LS="查看所有空间"

_I18N_en_HELP_EXAMPLE_ATTACH="Reconnect to running session after SSH disconnect"
_I18N_zh_HELP_EXAMPLE_ATTACH="SSH 断线后重连运行中的会话"

_I18N_en_HELP_EXAMPLE_RM="Destroy Worktree (session+code+branch)"
_I18N_zh_HELP_EXAMPLE_RM="彻底销毁Worktree (删除会话+代码+分支)"

_I18N_en_HELP_CMD_DETAILS="For command details use:"
_I18N_zh_HELP_CMD_DETAILS="各命令详细用法请使用:"

_I18N_en_ERR_NOT_INITIALIZED="Project not initialized!"
_I18N_zh_ERR_NOT_INITIALIZED="项目尚未初始化！"

_I18N_en_ERR_RUN_INIT="Run the following to configure:"
_I18N_zh_ERR_RUN_INIT="请先运行以下命令进行配置:"

_I18N_en_ERR_UNKNOWN_CMD="Unknown command:"
_I18N_zh_ERR_UNKNOWN_CMD="未知命令:"

# === worktree_service ===

_I18N_en_ERR_NOT_GIT_REPO="Current directory is not a Git repository. Run 'git init' first or cd into a Git project."
_I18N_zh_ERR_NOT_GIT_REPO="当前目录不是 Git 仓库。请先运行 'git init' 或进入一个 Git 项目目录。"

_I18N_en_ERR_DANGEROUS_DIR="Refusing to initialize in '%s'. This is a system or home directory — initializing git here would cause severe performance issues (shell prompts with git status scan the entire directory tree). Please cd into a project directory first."
_I18N_zh_ERR_DANGEROUS_DIR="拒绝在 '%s' 中初始化。这是系统目录或用户主目录——在此初始化 git 会导致严重的性能问题（Shell 提示符的 git status 会扫描整个目录树）。请先 cd 到一个项目目录中。"

_I18N_en_ERR_HARD_GUARD_ROOT="Refusing to initialize in '%s'. Root directory is protected by a built-in hard guard."
_I18N_zh_ERR_HARD_GUARD_ROOT="拒绝在 '%s' 中初始化。根目录由内置硬保护规则阻止。"

_I18N_en_HINT_HARD_GUARD_ROOT="This is expected behavior (not a config bug). Root (/) cannot be overridden by whitelist or dangerous_paths_extra."
_I18N_zh_HINT_HARD_GUARD_ROOT="这是预期行为（不是配置 bug）。根目录 (/) 不能被白名单或 dangerous_paths_extra 覆盖。"

_I18N_en_ASK_INIT_GIT_REPO="No Git repository found. Initialize one now?"
_I18N_zh_ASK_INIT_GIT_REPO="未检测到 Git 仓库。是否立即初始化？"

_I18N_en_MSG_GIT_INIT_SUCCESS="Git repository initialized."
_I18N_zh_MSG_GIT_INIT_SUCCESS="Git 仓库已初始化。"

_I18N_en_MSG_GIT_INIT_REQUIRED="A Git repository is required for project-level operations."
_I18N_zh_MSG_GIT_INIT_REQUIRED="项目级操作需要 Git 仓库。"

_I18N_en_ERR_GIT_INIT_FAILED="Failed to initialize git repository."
_I18N_zh_ERR_GIT_INIT_FAILED="初始化 Git 仓库失败。"

_I18N_en_ERR_GIT_COMMIT_FAILED="Failed to create initial commit."
_I18N_zh_ERR_GIT_COMMIT_FAILED="创建初始提交失败。"

_I18N_en_MSG_GIT_NO_COMMITS="Git repository has no commits yet. A commit is required for worktree operations."
_I18N_zh_MSG_GIT_NO_COMMITS="Git 仓库还没有任何提交。Worktree 操作需要至少一个提交。"

_I18N_en_ASK_CREATE_INITIAL_COMMIT="Create an initial empty commit?"
_I18N_zh_ASK_CREATE_INITIAL_COMMIT="是否创建一个初始空提交？"

_I18N_en_ERR_NOT_IN_WHITELIST="Directory '%s' is not in the git init whitelist. Git initialization is not allowed here."
_I18N_zh_ERR_NOT_IN_WHITELIST="目录 '%s' 不在 git init 白名单中。不允许在此初始化 git。"

_I18N_en_HINT_ADD_WHITELIST="Add this directory to the whitelist: workhive config git-whitelist add <path>"
_I18N_zh_HINT_ADD_WHITELIST="将此目录加入白名单: workhive config git-whitelist add <路径>"

_I18N_en_HINT_MANUAL_GIT_INIT="To initialize manually, run: git init && git commit --allow-empty -m 'Initial commit'"
_I18N_zh_HINT_MANUAL_GIT_INIT="手动初始化请执行: git init && git commit --allow-empty -m 'Initial commit'"

_I18N_en_MSG_GIT_INIT_PREVIEW="This will: 1) Run 'git init'  2) Apply .gitignore template  3) Create initial commit"
_I18N_zh_MSG_GIT_INIT_PREVIEW="将执行: 1) 运行 'git init'  2) 应用 .gitignore 模板  3) 创建初始提交"

_I18N_en_MSG_NO_GLOBAL_CONFIG="No global configuration found. workhive requires global configuration for path policies and personalization."
_I18N_zh_MSG_NO_GLOBAL_CONFIG="未检测到全局配置。workhive 需要全局配置来管理路径策略和个性化设置。"

_I18N_en_ASK_SETUP_GLOBAL_CONFIG="Set up global configuration now?"
_I18N_zh_ASK_SETUP_GLOBAL_CONFIG="是否立即进行全局配置？"

_I18N_en_HINT_SETUP_GLOBAL="Run: workhive init --global"
_I18N_zh_HINT_SETUP_GLOBAL="请运行: workhive init --global"

_I18N_en_MSG_GLOBAL_CONFIG_DONE_CONTINUE="Global configuration complete. Continuing with project initialization..."
_I18N_zh_MSG_GLOBAL_CONFIG_DONE_CONTINUE="全局配置已完成，继续进行项目初始化..."

_I18N_en_MSG_GLOBAL_CONFIG_DONE_RERUN="Global configuration complete! Please re-run your command:"
_I18N_zh_MSG_GLOBAL_CONFIG_DONE_RERUN="全局配置已完成！请重新执行你的命令："

_I18N_en_GIT_WHITELIST_ADDED="Added to git init whitelist: %s"
_I18N_zh_GIT_WHITELIST_ADDED="已加入 git init 白名单: %s"

_I18N_en_GIT_WHITELIST_REMOVED="Removed from git init whitelist: %s"
_I18N_zh_GIT_WHITELIST_REMOVED="已从 git init 白名单移除: %s"

_I18N_en_GIT_WHITELIST_NOT_FOUND="Path not found in whitelist: %s"
_I18N_zh_GIT_WHITELIST_NOT_FOUND="白名单中未找到该路径: %s"

_I18N_en_GIT_WHITELIST_ALREADY_EXISTS="Path already exists in whitelist: %s"
_I18N_zh_GIT_WHITELIST_ALREADY_EXISTS="白名单中已存在该路径: %s"

_I18N_en_GIT_WHITELIST_UPDATED="Updated whitelist path: %s -> %s"
_I18N_zh_GIT_WHITELIST_UPDATED="已更新白名单路径: %s -> %s"

_I18N_en_GIT_WHITELIST_EMPTY="Git init whitelist is empty. All non-dangerous directories are allowed."
_I18N_zh_GIT_WHITELIST_EMPTY="Git init 白名单为空。所有非危险目录均可使用。"

_I18N_en_GIT_WHITELIST_TITLE="Git init whitelist paths:"
_I18N_zh_GIT_WHITELIST_TITLE="Git init 白名单路径:"

_I18N_en_GIT_WHITELIST_ERR_TTY_REQUIRED="Interactive git-whitelist manager requires a TTY."
_I18N_zh_GIT_WHITELIST_ERR_TTY_REQUIRED="交互式 git-whitelist 管理器需要 TTY 环境。"

_I18N_en_GIT_WHITELIST_HINT_NON_INTERACTIVE="Use: workhive config git-whitelist list|add <path>|edit <old-path> <new-path>|remove <path>"
_I18N_zh_GIT_WHITELIST_HINT_NON_INTERACTIVE="请使用: workhive config git-whitelist list|add <path>|edit <旧路径> <新路径>|remove <path>"

_I18N_en_GIT_WHITELIST_MENU_TITLE="Git Whitelist Manager"
_I18N_zh_GIT_WHITELIST_MENU_TITLE="Git 白名单管理器"

_I18N_en_GIT_WHITELIST_MENU_LIST="List whitelist"
_I18N_zh_GIT_WHITELIST_MENU_LIST="查看白名单"

_I18N_en_GIT_WHITELIST_MENU_ADD="Add path"
_I18N_zh_GIT_WHITELIST_MENU_ADD="添加路径"

_I18N_en_GIT_WHITELIST_MENU_EDIT="Edit path"
_I18N_zh_GIT_WHITELIST_MENU_EDIT="修改路径"

_I18N_en_GIT_WHITELIST_MENU_REMOVE="Remove path"
_I18N_zh_GIT_WHITELIST_MENU_REMOVE="删除路径"

_I18N_en_GIT_WHITELIST_MENU_DONE="Done"
_I18N_zh_GIT_WHITELIST_MENU_DONE="完成"

_I18N_en_GIT_WHITELIST_PROMPT_ADD_PATH="Path to add:"
_I18N_zh_GIT_WHITELIST_PROMPT_ADD_PATH="待添加路径:"

_I18N_en_GIT_WHITELIST_PROMPT_EDIT_OLD_PATH="Path to edit (old):"
_I18N_zh_GIT_WHITELIST_PROMPT_EDIT_OLD_PATH="待修改路径(旧):"

_I18N_en_GIT_WHITELIST_PROMPT_EDIT_NEW_PATH="New path:"
_I18N_zh_GIT_WHITELIST_PROMPT_EDIT_NEW_PATH="新路径:"

_I18N_en_GIT_WHITELIST_PROMPT_REMOVE_PATH="Path to remove:"
_I18N_zh_GIT_WHITELIST_PROMPT_REMOVE_PATH="待删除路径:"

_I18N_en_ERR_BRANCH_EMPTY="Branch name cannot be empty"
_I18N_zh_ERR_BRANCH_EMPTY="分支名不能为空"

_I18N_en_ERR_BRANCH_SPACES="Branch name cannot contain spaces:"
_I18N_zh_ERR_BRANCH_SPACES="分支名不能包含空格:"

_I18N_en_ERR_BRANCH_ILLEGAL_CHARS="Branch name contains path traversal (..):"
_I18N_zh_ERR_BRANCH_ILLEGAL_CHARS="分支名包含路径遍历 (..):"

_I18N_en_ERR_BRANCH_INVALID="Branch name allows only letters, numbers, hyphens, underscores, dots and slashes:"
_I18N_zh_ERR_BRANCH_INVALID="分支名只允许字母、数字、连字符、下划线、点号和斜杠:"

_I18N_en_ASK_CREATE_WT="Worktree '%s' does not exist. Create it?"
_I18N_zh_ASK_CREATE_WT="Worktree '%s' 不存在。是否创建？"

_I18N_en_INFO_WT_EXISTS="Worktree already exists:"
_I18N_zh_INFO_WT_EXISTS="Worktree 已存在:"

_I18N_en_INFO_WT_CREATING="Creating Worktree:"
_I18N_zh_INFO_WT_CREATING="创建 Worktree:"

_I18N_en_INFO_WT_CREATED="Worktree created:"
_I18N_zh_INFO_WT_CREATED="Worktree 已创建:"

_I18N_en_ERR_WT_NOT_FOUND="Worktree not found:"
_I18N_zh_ERR_WT_NOT_FOUND="Worktree 不存在:"

_I18N_en_INFO_WT_REMOVING="Removing Worktree:"
_I18N_zh_INFO_WT_REMOVING="删除 Worktree:"

_I18N_en_INFO_WT_REMOVED="Worktree removed:"
_I18N_zh_INFO_WT_REMOVED="Worktree 已删除:"

_I18N_en_ERR_WT_REMOVE_FAILED="Worktree removal failed"
_I18N_zh_ERR_WT_REMOVE_FAILED="Worktree 删除失败"

_I18N_en_ERR_WT_CORRUPTED="Worktree corrupted, please fix or remove manually"
_I18N_zh_ERR_WT_CORRUPTED="Worktree 损坏,请手动修复或删除"

_I18N_en_ERR_WT_CREATE_FAILED="Worktree creation failed. If the branch already exists, try: workhive open <branch>"
_I18N_zh_ERR_WT_CREATE_FAILED="Worktree 创建失败。如果分支已存在, 请尝试: workhive open <branch>"

_I18N_en_INFO_WORKING_DIR="Working directory:"
_I18N_zh_INFO_WORKING_DIR="工作目录:"

_I18N_en_MSG_FORCE_DESTROY_SESSION="Force mode: destroying old session..."
_I18N_zh_MSG_FORCE_DESTROY_SESSION="强制模式: 销毁旧会话..."
_I18N_en_WARN_FORCE_SNAPSHOT_PURGE_FAILED="Failed to purge snapshot files before force rebuild"
_I18N_zh_WARN_FORCE_SNAPSHOT_PURGE_FAILED="强制重建前清理快照文件失败"
_I18N_en_ERR_FORCE_RECREATE_CURRENT_SESSION="Force recreate cannot run from inside the target tmux session."
_I18N_zh_ERR_FORCE_RECREATE_CURRENT_SESSION="不能在目标 tmux 会话内部执行强制重建。"
_I18N_en_HINT_FORCE_RECREATE_OUTSIDE_SESSION="Detach first or run the command from outside that workspace session."
_I18N_zh_HINT_FORCE_RECREATE_OUTSIDE_SESSION="请先分离该会话，或在该工作空间会话外执行命令。"

_I18N_en_MSG_FORCE_LAYOUT_REBUILD="Force layout: rebuilding layout from config"
_I18N_zh_MSG_FORCE_LAYOUT_REBUILD="强制布局: 按配置文件重建布局"

_I18N_en_ERR_SESSION_CREATE_FAILED="Session creation failed. Check if tmux is running: tmux ls"
_I18N_zh_ERR_SESSION_CREATE_FAILED="会话创建失败。请检查 tmux 是否正常: tmux ls"

_I18N_en_MSG_SESSION_RECREATED="Session recreated:"
_I18N_zh_MSG_SESSION_RECREATED="会话已重建:"

_I18N_en_MSG_SESSION_EXISTS="Session exists:"
_I18N_zh_MSG_SESSION_EXISTS="会话已存在:"

_I18N_en_WARN_SESSION_CORRUPTED="Session may be corrupted, will try to recreate"
_I18N_zh_WARN_SESSION_CORRUPTED="会话可能损坏,将尝试重新创建"

_I18N_en_ASK_OPEN_SAFE_SESSION_REPAIR="Session health check failed. Recreate session now? (this will close the existing tmux session)"
_I18N_zh_ASK_OPEN_SAFE_SESSION_REPAIR="会话健康检查失败。是否立即重建会话？(将关闭现有 tmux 会话)"

_I18N_en_ERR_OPEN_SAFE_SESSION_REPAIR_NEEDS_FORCE="Session repair requires destructive recreate in non-interactive mode."
_I18N_zh_ERR_OPEN_SAFE_SESSION_REPAIR_NEEDS_FORCE="非交互模式下修复会话需要破坏性重建。"

_I18N_en_WARN_KILLING_CURRENT_SESSION="Killing current session %s (your terminal will disconnect)"
_I18N_zh_WARN_KILLING_CURRENT_SESSION="正在关闭当前会话 %s (当前终端将断开)"

_I18N_en_ERR_SESSION_RECREATE_FAILED="Session recreate failed"
_I18N_zh_ERR_SESSION_RECREATE_FAILED="会话重建失败"

_I18N_en_MSG_LAYOUT_EXISTS_SKIP="Layout exists, skipping command re-send"
_I18N_zh_MSG_LAYOUT_EXISTS_SKIP="布局已存在,跳过命令重发"

_I18N_en_MSG_LAYOUT_DAMAGED_RECREATE="Layout damaged, recreating session"
_I18N_zh_MSG_LAYOUT_DAMAGED_RECREATE="布局损坏,重建会话"

_I18N_en_MSG_CREATE_LAYOUT="Creating layout"
_I18N_zh_MSG_CREATE_LAYOUT="创建布局"

_I18N_en_ERR_LAYOUT_CREATE_FAILED="Layout creation failed"
_I18N_zh_ERR_LAYOUT_CREATE_FAILED="布局创建失败"

_I18N_en_WARN_LAYOUT_INCOMPLETE="Session created, but layout incomplete"
_I18N_zh_WARN_LAYOUT_INCOMPLETE="会话已创建,但布局未完成"

_I18N_en_WARN_LAYOUT_REBUILD_ROLLBACK="Layout rebuild failed, restored previous snapshot session"
_I18N_zh_WARN_LAYOUT_REBUILD_ROLLBACK="布局重建失败，已回滚恢复到之前快照会话"

_I18N_en_ASK_OPEN_SAFE_LAYOUT_REPAIR="Layout is insufficient for current config. Rebuild session layout now? (this may recreate session)"
_I18N_zh_ASK_OPEN_SAFE_LAYOUT_REPAIR="当前布局不足以匹配配置。是否立即重建会话布局？(可能会重建会话)"

_I18N_en_ERR_OPEN_SAFE_LAYOUT_REPAIR_NEEDS_FORCE="Layout repair requires destructive rebuild in non-interactive mode."
_I18N_zh_ERR_OPEN_SAFE_LAYOUT_REPAIR_NEEDS_FORCE="非交互模式下修复布局需要破坏性重建。"

_I18N_en_HINT_OPEN_FORCE_REPAIR="Re-run with 'workhive open -f <branch>' to force recreate from config.yaml."
_I18N_zh_HINT_OPEN_FORCE_REPAIR="请使用 'workhive open -f <branch>' 重新执行，以按 config.yaml 强制重建。"

_I18N_en_WARN_OPEN_SAFE_LAYOUT_REPAIR_SKIPPED="Skipped destructive layout repair, keeping current session layout."
_I18N_zh_WARN_OPEN_SAFE_LAYOUT_REPAIR_SKIPPED="已跳过破坏性布局修复，保持当前会话布局。"

_I18N_en_MSG_ATTACH_EXISTING_SESSION_CONTINUE="Continue attaching to existing session."
_I18N_zh_MSG_ATTACH_EXISTING_SESSION_CONTINUE="继续附加到现有会话。"

_I18N_en_MSG_LAYOUT_CREATED="Layout created"
_I18N_zh_MSG_LAYOUT_CREATED="布局已创建"

_I18N_en_MSG_PANE_SUMMARY_UNAVAILABLE="Pane summary unavailable (session is ready; use 'workhive pane list' to inspect panes)"
_I18N_zh_MSG_PANE_SUMMARY_UNAVAILABLE="窗格摘要暂不可用（会话已就绪，可使用 'workhive pane list' 查看）"

_I18N_en_INFO_LAYOUT_SOURCE="Layout source"
_I18N_zh_INFO_LAYOUT_SOURCE="布局来源"
_I18N_en_INFO_LAYOUT_SOURCE_SESSION="existing tmux session"
_I18N_zh_INFO_LAYOUT_SOURCE_SESSION="已存在 tmux 会话"
_I18N_en_INFO_LAYOUT_SOURCE_SNAPSHOT="snapshot restore"
_I18N_zh_INFO_LAYOUT_SOURCE_SNAPSHOT="快照恢复"
_I18N_en_INFO_LAYOUT_SOURCE_CONFIG="config.yaml rebuild"
_I18N_zh_INFO_LAYOUT_SOURCE_CONFIG="config.yaml 重建"

_I18N_en_MSG_APPLY_CMD_CONFIG="Applying command config"
_I18N_zh_MSG_APPLY_CMD_CONFIG="应用命令配置"

_I18N_en_MSG_CMD_CONFIG_APPLIED="Command config applied"
_I18N_zh_MSG_CMD_CONFIG_APPLIED="命令配置已应用"

_I18N_en_WARN_CMD_CONFIG_FAILED="Command config apply failed (can start tools manually)"
_I18N_zh_WARN_CMD_CONFIG_FAILED="命令配置应用失败 (可手动启动工具)"

# === config command (config_command_show_summary, etc.) ===

_I18N_en_ERR_CONFIG_NOT_FOUND="Config file not found:"
_I18N_zh_ERR_CONFIG_NOT_FOUND="配置文件不存在:"

_I18N_en_ERR_RUN_INIT_FIRST="Please run: workhive init"
_I18N_zh_ERR_RUN_INIT_FIRST="请先运行: workhive init"

_I18N_en_TITLE_CONFIG_SUMMARY="Current config summary"
_I18N_zh_TITLE_CONFIG_SUMMARY="当前配置摘要"

_I18N_en_LABEL_LAYOUT_MODE="Layout mode:"
_I18N_zh_LABEL_LAYOUT_MODE="布局模式:"

_I18N_en_LABEL_TMUX_CONFIG="tmux config:"
_I18N_zh_LABEL_TMUX_CONFIG="tmux 配置:"

_I18N_en_LABEL_SESSION_PREFIX="Session prefix:"
_I18N_zh_LABEL_SESSION_PREFIX="Session 前缀:"

_I18N_en_LABEL_MOUSE="Mouse support:"
_I18N_zh_LABEL_MOUSE="鼠标支持:"


_I18N_en_LABEL_THEME="Theme:"
_I18N_zh_LABEL_THEME="主题:"

_I18N_en_LABEL_WT_MANAGEMENT="Worktree management:"
_I18N_zh_LABEL_WT_MANAGEMENT="Worktree 管理:"

_I18N_en_LABEL_WORKTREE="Worktree:"
_I18N_zh_LABEL_WORKTREE="Worktree:"

_I18N_en_LABEL_BASE_PATH="Base path:"
_I18N_zh_LABEL_BASE_PATH="基础路径:"

_I18N_en_LABEL_AUTO_CREATE="Auto create:"
_I18N_zh_LABEL_AUTO_CREATE="自动创建:"

_I18N_en_LABEL_AUTO_DELETE="Auto delete:"
_I18N_zh_LABEL_AUTO_DELETE="自动删除:"

_I18N_en_LABEL_REMOTE_CONFIG="Remote config:"
_I18N_zh_LABEL_REMOTE_CONFIG="远程连接配置:"

_I18N_en_LABEL_CMD="Command:"
_I18N_zh_LABEL_CMD="命令:"

_I18N_en_LABEL_CMD_EMPTY="<empty>"
_I18N_zh_LABEL_CMD_EMPTY="<空>"

_I18N_en_MSG_CONFIG_SAVED="Config saved and validated"
_I18N_zh_MSG_CONFIG_SAVED="配置文件已保存并验证通过"

_I18N_en_ERR_CONFIG_SYNTAX="Config syntax error!"
_I18N_zh_ERR_CONFIG_SYNTAX="配置文件语法错误！"

_I18N_en_ERR_CONFIG_SYNTAX_PREFIX="Config syntax error:"
_I18N_zh_ERR_CONFIG_SYNTAX_PREFIX="配置文件语法错误:"

_I18N_en_WARN_NO_YQ_SKIP="yq not installed, skipping syntax validation"
_I18N_zh_WARN_NO_YQ_SKIP="未安装 yq，跳过语法验证"

_I18N_en_INFO_OPEN_EDITOR="Opening config file in editor:"
_I18N_zh_INFO_OPEN_EDITOR="使用编辑器打开配置文件:"

# === config loader ===

_I18N_en_ERR_NO_YQ="yq not installed, cannot modify config"
_I18N_zh_ERR_NO_YQ="yq 未安装,无法修改配置"

_I18N_en_INFO_CONFIG_EXISTS="Config file already exists:"
_I18N_zh_INFO_CONFIG_EXISTS="配置文件已存在:"

_I18N_en_INFO_CREATING_CONFIG="Creating default config..."
_I18N_zh_INFO_CREATING_CONFIG="创建默认配置文件..."

_I18N_en_INFO_CONFIG_CREATED="Config file created:"
_I18N_zh_INFO_CONFIG_CREATED="配置文件已创建:"

# === tmux adapter ===

_I18N_en_DEBUG_TMUX_VERSION_FAIL="Cannot get tmux version, skipping conditional config"
_I18N_zh_DEBUG_TMUX_VERSION_FAIL="无法获取 tmux 版本,跳过条件配置"

# === plugins manager ===

_I18N_en_PLUGIN_LOADED="plugin(s) loaded"
_I18N_zh_PLUGIN_LOADED="个插件已加载"

_I18N_en_PLUGIN_LOAD_FAILED="plugin(s) failed to load"
_I18N_zh_PLUGIN_LOAD_FAILED="个插件加载失败"

_I18N_en_PLUGIN_VALIDATE_MISSING_PATH="plugin_validate: missing plugin file path"
_I18N_zh_PLUGIN_VALIDATE_MISSING_PATH="plugin_validate: 缺少插件文件路径"

_I18N_en_PLUGIN_FILE_NOT_EXISTS="Plugin file does not exist: %s"
_I18N_zh_PLUGIN_FILE_NOT_EXISTS="插件文件不存在: %s"

_I18N_en_PLUGIN_FILE_NOT_READABLE="Plugin file not readable: %s"
_I18N_zh_PLUGIN_FILE_NOT_READABLE="插件文件不可读: %s"

_I18N_en_PLUGIN_FILE_NO_SHEBANG="Plugin file missing shebang and not .sh file: %s"
_I18N_zh_PLUGIN_FILE_NO_SHEBANG="插件文件缺少 shebang 且非 .sh 文件: %s"

_I18N_en_PLUGIN_ALREADY_LOADED="Plugin already loaded, skipping: %s"
_I18N_zh_PLUGIN_ALREADY_LOADED="插件已加载，跳过: %s"

_I18N_en_PLUGIN_LOADING="Loading plugin: %s (%s)"
_I18N_zh_PLUGIN_LOADING="加载插件: %s (%s)"

_I18N_en_PLUGIN_SYNTAX_ERROR="Plugin syntax error, skipping: %s (%s)"
_I18N_zh_PLUGIN_SYNTAX_ERROR="插件语法错误，跳过: %s (%s)"

_I18N_en_PLUGIN_LOAD_FAILED_FMT="Plugin load failed: %s (%s)"
_I18N_zh_PLUGIN_LOAD_FAILED_FMT="插件加载失败: %s (%s)"

_I18N_en_PLUGIN_EXEC_INIT="Executing plugin init: %s"
_I18N_zh_PLUGIN_EXEC_INIT="执行插件初始化: %s"

_I18N_en_PLUGIN_INIT_FAILED="Plugin init failed: %s"
_I18N_zh_PLUGIN_INIT_FAILED="插件初始化失败: %s"

_I18N_en_PLUGIN_LOAD_SUCCESS="Plugin loaded successfully: %s"
_I18N_zh_PLUGIN_LOAD_SUCCESS="插件加载成功: %s"

_I18N_en_PLUGIN_DIR_NOT_EXISTS="Plugin directory does not exist: %s (skipping)"
_I18N_zh_PLUGIN_DIR_NOT_EXISTS="插件目录不存在: %s (跳过)"

_I18N_en_PLUGIN_NO_FILES="No plugin files: %s"
_I18N_zh_PLUGIN_NO_FILES="无插件文件: %s"

_I18N_en_PLUGIN_NONE_LOADED="(no plugins loaded)"
_I18N_zh_PLUGIN_NONE_LOADED="(无已加载插件)"

# === terminal adapter factory ===

_I18N_en_ERR_ADAPTER_UNSUPPORTED="Error: Unsupported terminal adapter: %s"
_I18N_zh_ERR_ADAPTER_UNSUPPORTED="Error: 不支持的终端适配器: %s"

_I18N_en_ERR_ADAPTER_NOT_AVAILABLE="Error: Terminal adapter %s not available, please install first"
_I18N_zh_ERR_ADAPTER_NOT_AVAILABLE="Error: 终端适配器 %s 不可用,请先安装"

# === open command (additional) ===

_I18N_en_HELP_USAGE_PREFIX="Usage:"
_I18N_zh_HELP_USAGE_PREFIX="用法:"

_I18N_en_MSG_CREATE_NEW_SPACE="Create new workspace:"
_I18N_zh_MSG_CREATE_NEW_SPACE="创建新空间:"

_I18N_en_MSG_CREATE_NEW_WT="Create new Worktree"
_I18N_zh_MSG_CREATE_NEW_WT="创建新 Worktree"

_I18N_en_MSG_MAIN_DIR="Main directory"
_I18N_zh_MSG_MAIN_DIR="主目录"

_I18N_en_MSG_ACTIVE="Active"
_I18N_zh_MSG_ACTIVE="活跃"

_I18N_en_MSG_CREATED="Created"
_I18N_zh_MSG_CREATED="已创建"

_I18N_en_MSG_SELECT_OR_CREATE_WT="Select or create Worktree"
_I18N_zh_MSG_SELECT_OR_CREATE_WT="选择或创建 Worktree"

_I18N_en_MSG_ENTER_BRANCH="Enter branch name:"
_I18N_zh_MSG_ENTER_BRANCH="输入分支名:"

_I18N_en_MSG_AVAILABLE_SESSIONS="Available sessions:"
_I18N_zh_MSG_AVAILABLE_SESSIONS="可用的会话:"

_I18N_en_MSG_NO_ACTIVE_SESSIONS="(no active sessions)"
_I18N_zh_MSG_NO_ACTIVE_SESSIONS="(无活跃会话)"

_I18N_en_MSG_ATTACH_PROMPT="Attach to session:"
_I18N_zh_MSG_ATTACH_PROMPT="附加到会话:"

_I18N_en_INFO_CREATE_NEW_SESSION="Create new session:"
_I18N_zh_INFO_CREATE_NEW_SESSION="创建新会话:"

_I18N_en_MSG_WORKSPACE_CREATED="Workspace created!"
_I18N_zh_MSG_WORKSPACE_CREATED="工作空间已创建!"

_I18N_en_MSG_USE_ATTACH="Use the following to attach:"
_I18N_zh_MSG_USE_ATTACH="使用以下命令附加到会话:"

# === remove command ===

_I18N_en_TITLE_REMOVE_WT="Destroy Worktree:"
_I18N_zh_TITLE_REMOVE_WT="彻底销毁Worktree:"

_I18N_en_MSG_AVAILABLE_SPACES="Available workspaces:"
_I18N_zh_MSG_AVAILABLE_SPACES="可用的空间:"

_I18N_en_MSG_REMOVE_STEP_1="Step 1/3: Closing tmux session..."
_I18N_zh_MSG_REMOVE_STEP_1="步骤 1/3: 关闭 tmux 会话..."

_I18N_en_CONFIRM_KILL_SESSION="This will close the tmux session and all panes. Continue?"
_I18N_zh_CONFIRM_KILL_SESSION="这将关闭 tmux 会话及所有 pane，是否继续？"

_I18N_en_MSG_SESSION_CLOSED="Session closed:"
_I18N_zh_MSG_SESSION_CLOSED="会话已关闭:"

_I18N_en_MSG_SESSION_CLOSE_FAILED="Session close failed (may already be gone)"
_I18N_zh_MSG_SESSION_CLOSE_FAILED="会话关闭失败 (可能已不存在)"

_I18N_en_REMOVE_ERR_SESSIONS_REMAIN="Session cleanup incomplete: some sessions are still running"
_I18N_zh_REMOVE_ERR_SESSIONS_REMAIN="会话清理不完整: 仍有会话在运行"

_I18N_en_REMOVE_INFO_RETRY_FORCE="Please close remaining sessions and retry remove"
_I18N_zh_REMOVE_INFO_RETRY_FORCE="请先关闭残留会话后重试 remove"

_I18N_en_MSG_SESSION_NOT_EXIST="Step 1/3: Session does not exist, skipping"
_I18N_zh_MSG_SESSION_NOT_EXIST="步骤 1/3: 会话不存在,跳过"

_I18N_en_MSG_WT_NOT_EXIST_SKIP="Step 2/3: Worktree does not exist, skipping"
_I18N_zh_MSG_WT_NOT_EXIST_SKIP="步骤 2/3: Worktree 不存在,跳过"

_I18N_en_MSG_DESTROY_DONE="Destroy complete!"
_I18N_zh_MSG_DESTROY_DONE="销毁完成!"

_I18N_en_MSG_KEEP_WT="Step 2/3: Keeping Worktree (--keep-worktree)"
_I18N_zh_MSG_KEEP_WT="步骤 2/3: 保留 Worktree (--keep-worktree)"

_I18N_en_MSG_WT_PATH="Worktree path:"
_I18N_zh_MSG_WT_PATH="Worktree 路径:"

_I18N_en_MSG_REMOVE_WT="Step 2/3: Removing Worktree..."
_I18N_zh_MSG_REMOVE_WT="步骤 2/3: 删除 Worktree..."

_I18N_en_ERR_CANNOT_ACCESS_WT="Cannot access Worktree:"
_I18N_zh_ERR_CANNOT_ACCESS_WT="无法访问 Worktree:"

_I18N_en_WARN_WT_UNCOMMITTED="Worktree has uncommitted changes!"
_I18N_zh_WARN_WT_UNCOMMITTED="Worktree 有未提交的更改!"

_I18N_en_CONFIRM_DESTROY_WT="Confirm destroy? (uncommitted changes and branch will be lost)"
_I18N_zh_CONFIRM_DESTROY_WT="确认彻底销毁? (未提交更改及关联分支将一并丢失)"

_I18N_en_WARN_BRANCH_UNMERGED="Branch has unmerged commits (not merged into main branch)!"
_I18N_zh_WARN_BRANCH_UNMERGED="分支有未合并的提交 (未合并到主分支)!"

_I18N_en_CONFIRM_DELETE_UNMERGED="This branch has unmerged commits. Delete anyway? (commits will be lost)"
_I18N_zh_CONFIRM_DELETE_UNMERGED="该分支有未合并的提交。仍然删除？(提交将丢失)"

_I18N_en_MSG_TRY_FORCE="Try force destroy:"
_I18N_zh_MSG_TRY_FORCE="尝试强制销毁:"

_I18N_en_MSG_REMOVE_STEP_3="Step 3/3: Cleaning Git branch..."
_I18N_zh_MSG_REMOVE_STEP_3="步骤 3/3: 清理 Git 分支..."

_I18N_en_MSG_BRANCH_DELETED="Git branch deleted:"
_I18N_zh_MSG_BRANCH_DELETED="Git 分支已彻底删除:"

_I18N_en_WARN_BRANCH_DELETE_FAILED="Git branch delete failed (may have unmerged changes or in use)"
_I18N_zh_WARN_BRANCH_DELETE_FAILED="Git 分支删除失败 (可能存在未合并的更改或正在被使用)"

_I18N_en_INFO_CHECK_BRANCH="Check manually: git branch -D"
_I18N_zh_INFO_CHECK_BRANCH="建议手动检查: git branch -D"

# === destroy command (additional) ===

_I18N_en_ERR_NO_WORKHIVE_DIR=".workhive/ directory not found, project not initialized"
_I18N_zh_ERR_NO_WORKHIVE_DIR="未找到 .workhive/ 目录,项目尚未初始化"

_I18N_en_INFO_USE_INIT="Use 'workhive init' to initialize"
_I18N_zh_INFO_USE_INIT="使用 'workhive init' 初始化项目"

_I18N_en_DESTROY_INFO_CLEANUP_SESSIONS_ONLY="Proceeding with session cleanup only (workhive directory already removed)"
_I18N_zh_DESTROY_INFO_CLEANUP_SESSIONS_ONLY="将仅执行会话清理（workhive 目录已被删除）"

_I18N_en_DESTROY_INFO_WORKHIVE_ALREADY_REMOVED=".workhive directory already removed, skip directory deletion"
_I18N_zh_DESTROY_INFO_WORKHIVE_ALREADY_REMOVED=".workhive 目录已不存在，跳过目录删除"

_I18N_en_ERR_CANNOT_DESTROY_IN_WT="Cannot run destroy inside Worktree directory"
_I18N_zh_ERR_CANNOT_DESTROY_IN_WT="不能在 Worktree 目录内执行 destroy"

_I18N_en_INFO_CD_TO_ROOT="Please cd to project root: cd"
_I18N_zh_INFO_CD_TO_ROOT="请先 cd 到项目根目录: cd"

_I18N_en_MSG_TMUX_SESSIONS="tmux sessions"
_I18N_zh_MSG_TMUX_SESSIONS="tmux 会话"

_I18N_en_MSG_CONFIG_DIR="Config directory:"
_I18N_zh_MSG_CONFIG_DIR="配置目录:"

_I18N_en_WARN_DIRTY_WT="Worktree(s) have uncommitted changes, unrecoverable after delete!"
_I18N_zh_WARN_DIRTY_WT="个 Worktree 有未提交更改,删除后不可恢复!"

_I18N_en_MSG_SESSIONS_CLOSED="sessions closed"
_I18N_zh_MSG_SESSIONS_CLOSED="个会话已关闭"

_I18N_en_MSG_NO_SESSIONS="No active sessions"
_I18N_zh_MSG_NO_SESSIONS="无活跃会话"

_I18N_en_DESTROY_ERR_SESSIONS_REMAIN="Destroy aborted: some project sessions are still running"
_I18N_zh_DESTROY_ERR_SESSIONS_REMAIN="销毁已中止: 仍有项目会话在运行"

_I18N_en_DESTROY_WARN_SESSIONS_REMAIN="Some project sessions could not be closed automatically"
_I18N_zh_DESTROY_WARN_SESSIONS_REMAIN="部分项目会话未能自动关闭"

_I18N_en_DESTROY_INFO_MANUAL_SESSION_CLEANUP="Manual cleanup commands:"
_I18N_zh_DESTROY_INFO_MANUAL_SESSION_CLEANUP="手动清理命令:"

_I18N_en_MSG_WT_DELETED="Worktree(s) deleted"
_I18N_zh_MSG_WT_DELETED="个 Worktree 已删除"

_I18N_en_MSG_NO_WT="No Worktrees"
_I18N_zh_MSG_NO_WT="无 Worktree"

_I18N_en_ERR_DELETE_FAILED="Failed to delete .workhive/"
_I18N_zh_ERR_DELETE_FAILED="删除 .workhive/ 失败"

_I18N_en_INFO_MANUAL_DELETE="Please delete manually: rm -rf -- <path>"
_I18N_zh_INFO_MANUAL_DELETE="请手动删除: rm -rf -- <路径>"

_I18N_en_MSG_DELETED="Deleted:"
_I18N_zh_MSG_DELETED="已删除:"

_I18N_en_MSG_PROJECT_RESET="Project reset to uninitialized state"
_I18N_zh_MSG_PROJECT_RESET="项目已恢复到未初始化状态"

_I18N_en_MSG_USE_INIT_REINIT="Use 'workhive init' to reinitialize"
_I18N_zh_MSG_USE_INIT_REINIT="使用 'workhive init' 重新初始化"

_I18N_en_TITLE_DESTROY_GLOBAL="Destroy global configuration"
_I18N_zh_TITLE_DESTROY_GLOBAL="删除全局配置"

_I18N_en_MSG_GLOBAL_RESOURCES_TO_DELETE="The following global resources will be permanently deleted:"
_I18N_zh_MSG_GLOBAL_RESOURCES_TO_DELETE="以下全局资源将被永久删除:"

_I18N_en_MSG_GLOBAL_CONFIG_DIR="Global config directory:"
_I18N_zh_MSG_GLOBAL_CONFIG_DIR="全局配置目录:"

_I18N_en_CONFIRM_DESTROY_GLOBAL="Confirm delete global configuration? (irreversible)"
_I18N_zh_CONFIRM_DESTROY_GLOBAL="确认删除全局配置? (此操作不可逆)"

_I18N_en_MSG_GLOBAL_DESTROY_COMPLETE="Global configuration deleted!"
_I18N_zh_MSG_GLOBAL_DESTROY_COMPLETE="全局配置已删除!"

_I18N_en_MSG_USE_INIT_GLOBAL_REINIT="Use 'workhive init --global' to reinitialize global config"
_I18N_zh_MSG_USE_INIT_GLOBAL_REINIT="使用 'workhive init --global' 重新初始化全局配置"

# === list command (additional) ===

_I18N_en_MSG_LEGEND_EXISTS="exists"
_I18N_zh_MSG_LEGEND_EXISTS="存在"

_I18N_en_MSG_LEGEND_NOT_EXISTS="does not exist"
_I18N_zh_MSG_LEGEND_NOT_EXISTS="不存在"

_I18N_en_MSG_ACTIONS="Actions:"
_I18N_zh_MSG_ACTIONS="操作:"

_I18N_en_MSG_ENTER_WORKSPACE="Enter workspace:"
_I18N_zh_MSG_ENTER_WORKSPACE="进入工作空间:"

_I18N_en_MSG_DESTROY_WT="Destroy Worktree:"
_I18N_zh_MSG_DESTROY_WT="彻底销毁Worktree:"

_I18N_en_MSG_CLOSE_SESSION_ONLY="Close session only:"
_I18N_zh_MSG_CLOSE_SESSION_ONLY="仅关闭会话:"

# === pane command (additional) ===

_I18N_en_INFO_USE_OPEN="Use 'workhive open <worktree>' to enter workspace"
_I18N_zh_INFO_USE_OPEN="请先使用 'workhive open <worktree>' 进入工作空间"

_I18N_en_ERR_CANNOT_DETECT_TMUX="Cannot detect current tmux session"
_I18N_zh_ERR_CANNOT_DETECT_TMUX="无法检测当前 tmux 会话信息"

_I18N_en_INFO_SESSION="Session:"
_I18N_zh_INFO_SESSION="会话:"

_I18N_en_INFO_CURRENT_PANE="Current Pane:"
_I18N_zh_INFO_CURRENT_PANE="当前 Pane:"

_I18N_en_INFO_TARGET_MAIN="Target: Main directory"
_I18N_zh_INFO_TARGET_MAIN="目标: 主目录"

_I18N_en_INFO_LAUNCH="Launch:"
_I18N_zh_INFO_LAUNCH="启动:"

_I18N_en_INFO_TARGET="Target:"
_I18N_zh_INFO_TARGET="目标:"

_I18N_en_ERR_LAST_PANE="Cannot close the last pane in the session"
_I18N_zh_ERR_LAST_PANE="无法关闭会话中的最后一个 pane"

_I18N_en_INFO_USE_DESTROY="Use 'workhive rm' to remove the workspace instead"
_I18N_zh_INFO_USE_DESTROY="请改用 'workhive rm' 移除该工作空间"

_I18N_en_CONFIRM_CLOSE_PANE="Close this pane? (Enter to restart dead pane instead)"
_I18N_zh_CONFIRM_CLOSE_PANE="确认关闭此 pane？(如果进程已退出，按 Enter 可重启)"

_I18N_en_INFO_CLOSING_PANE="Closing current pane..."
_I18N_zh_INFO_CLOSING_PANE="正在关闭当前 pane..."

_I18N_en_INFO_SPLITTING_PANE="Splitting pane below..."
_I18N_zh_INFO_SPLITTING_PANE="在下方分割新 pane..."

_I18N_en_ERR_MISSING_CMD_VALUE="--cmd requires a value"
_I18N_zh_ERR_MISSING_CMD_VALUE="--cmd 需要一个值"

# === attach command (additional) ===

_I18N_en_INFO_SESSION_INFO="Session info:"
_I18N_zh_INFO_SESSION_INFO="会话信息:"
_I18N_en_INFO_SESSION_ADAPTER="Adapter"
_I18N_zh_INFO_SESSION_ADAPTER="适配器"
_I18N_en_INFO_SESSION_WINDOWS="Windows"
_I18N_zh_INFO_SESSION_WINDOWS="窗口数"
_I18N_en_INFO_SESSION_ATTACHED="Attached"
_I18N_zh_INFO_SESSION_ATTACHED="已附加"
_I18N_en_LABEL_YES="yes"
_I18N_zh_LABEL_YES="是"
_I18N_en_LABEL_NO="no"
_I18N_zh_LABEL_NO="否"

_I18N_en_ERR_SESSION_CORRUPTED="Session corrupted, cannot attach"
_I18N_zh_ERR_SESSION_CORRUPTED="会话损坏,无法附加"

_I18N_en_INFO_TRY_RECREATE="Try recreating session:"
_I18N_zh_INFO_TRY_RECREATE="尝试重新创建会话:"

_I18N_en_WARN_READONLY_NEEDS_TMUX3="Read-only mode requires tmux 3.0+, current version:"
_I18N_zh_WARN_READONLY_NEEDS_TMUX3="只读模式需要 tmux 3.0+,当前版本:"

_I18N_en_INFO_ATTACH_NORMAL="Will attach in normal mode"
_I18N_zh_INFO_ATTACH_NORMAL="将以普通模式附加"

_I18N_en_WARN_READONLY_TMUX_ONLY="Read-only mode is tmux only, screen not supported"
_I18N_zh_WARN_READONLY_TMUX_ONLY="只读模式仅 tmux 支持,screen 不支持"

_I18N_en_MSG_SELECT_ATTACH="Select session to attach"
_I18N_zh_MSG_SELECT_ATTACH="选择要附加的会话"

_I18N_en_MSG_SELECT_REMOVE="Select worktree to remove"
_I18N_zh_MSG_SELECT_REMOVE="选择要删除的 Worktree"

_I18N_en_MSG_NO_WT_TO_REMOVE="No worktrees found to remove"
_I18N_zh_MSG_NO_WT_TO_REMOVE="没有可删除的 Worktree"

# === open command (additional strings) ===

_I18N_en_INFO_MAIN_BRANCH_MODE="Branch is main repo current branch, switching to main directory mode"
_I18N_zh_INFO_MAIN_BRANCH_MODE="分支是主仓库当前分支,切换到主目录模式"

_I18N_en_MSG_LAYOUT_EXISTS="Layout exists"
_I18N_zh_MSG_LAYOUT_EXISTS="布局已存在"

_I18N_en_MSG_LAYOUT_DAMAGED="Layout damaged"
_I18N_zh_MSG_LAYOUT_DAMAGED="布局损坏"

_I18N_en_MSG_ASK_CONTINUE_ATTACH="Continue attaching to session?"
_I18N_zh_MSG_ASK_CONTINUE_ATTACH="是否继续附加到会话?"

_I18N_en_MSG_EDITOR_NOT_FOUND="Editor not found:"
_I18N_zh_MSG_EDITOR_NOT_FOUND="Editor 未找到:"

_I18N_en_INFO_TRY_VI="Try using vi"
_I18N_zh_INFO_TRY_VI="尝试使用 vi 编辑"

_I18N_en_MSG_REMOTE_PLACEHOLDER_FUTURE="Remote config is a placeholder, future versions will support:"
_I18N_zh_MSG_REMOTE_PLACEHOLDER_FUTURE="远程连接配置当前为预留功能,未来版本将支持:"

_I18N_en_MSG_REMOTE_USE_ATTACH="In current version, use 'workhive attach' to reconnect to an existing remote session"
_I18N_zh_MSG_REMOTE_USE_ATTACH="当前版本中,远程场景请使用 'workhive attach' 重新连接已有会话"

_I18N_en_TITLE_CONFIG_MANAGEMENT="workhive config management"
_I18N_zh_TITLE_CONFIG_MANAGEMENT="workhive 配置管理"

_I18N_en_INFO_CURRENT_CONFIG_FILE="Current config file:"
_I18N_zh_INFO_CURRENT_CONFIG_FILE="当前配置文件:"

_I18N_en_MSG_LAYOUT_MODE_HALF="half-split (50% left, 50% right)"
_I18N_zh_MSG_LAYOUT_MODE_HALF="half-split (左右对半 各 50%)"

_I18N_en_MSG_LAYOUT_MODE_LEFT="left-split (left 60% + right main 40%)"
_I18N_zh_MSG_LAYOUT_MODE_LEFT="left-split (左侧上下分栏 60% + 右侧主 Pane 40%)"

_I18N_en_MSG_LAYOUT_MODE_RIGHT="right-split (left main 40% + right 60%)"
_I18N_zh_MSG_LAYOUT_MODE_RIGHT="right-split (左侧主 Pane 40% + 右侧上下分栏 60%)"

_I18N_en_MSG_LAYOUT_MODE_QUAD="quad (four panes)"
_I18N_zh_MSG_LAYOUT_MODE_QUAD="quad (四分屏)"

_I18N_en_MSG_REMOTE_CURRENT="Current config: enabled ="
_I18N_zh_MSG_REMOTE_CURRENT="当前配置: 启用 ="

# === init_command (additional) ===

_I18N_en_MSG_GITIGNORE_CREATED="Created .gitignore and added"
_I18N_zh_MSG_GITIGNORE_CREATED="已创建 .gitignore 并添加"

_I18N_en_MSG_GITIGNORE_ADDED_PREFIX="Added"
_I18N_zh_MSG_GITIGNORE_ADDED_PREFIX="已将"

_I18N_en_MSG_GITIGNORE_ADDED_SUFFIX="to .gitignore"
_I18N_zh_MSG_GITIGNORE_ADDED_SUFFIX="添加到 .gitignore"

_I18N_en_MSG_GITIGNORE_ALREADY=".gitignore already contains"
_I18N_zh_MSG_GITIGNORE_ALREADY=".gitignore 中已包含"

_I18N_en_WARN_GITIGNORE_READONLY="Cannot write .gitignore (read-only). Please add '.workhive/' manually."
_I18N_zh_WARN_GITIGNORE_READONLY="无法写入 .gitignore (只读)。请手动添加 '.workhive/' 到 .gitignore。"

# === list count suffix ===

_I18N_en_MSG_COUNT="count"
_I18N_zh_MSG_COUNT="个"

# === worktree_service (force remove hint) ===

_I18N_en_INFO_USE_FORCE_REMOVE="Use force to delete:"
_I18N_zh_INFO_USE_FORCE_REMOVE="使用强制删除:"

# === session_service ===

_I18N_en_ERR_WORKDIR_NOT_FOUND="Working directory not found:"
_I18N_zh_ERR_WORKDIR_NOT_FOUND="工作目录不存在:"

_I18N_en_INFO_SESSION_EXISTS="Session exists:"
_I18N_zh_INFO_SESSION_EXISTS="会话已存在:"

_I18N_en_INFO_SESSION_CREATING="Creating session:"
_I18N_zh_INFO_SESSION_CREATING="创建会话:"

_I18N_en_INFO_SESSION_CREATED="Session created:"
_I18N_zh_INFO_SESSION_CREATED="会话已创建:"

_I18N_en_INFO_USE_OPEN_CREATE="Use 'workhive open <worktree>' to create session"
_I18N_zh_INFO_USE_OPEN_CREATE="使用 'workhive open <worktree>' 创建会话"

_I18N_en_INFO_ATTACHING_TO_SESSION="Attaching to session:"
_I18N_zh_INFO_ATTACHING_TO_SESSION="附加到会话:"
_I18N_en_WARN_ATTACH_SESSION_ENDED_FMT="Session ended while attaching: %s"
_I18N_zh_WARN_ATTACH_SESSION_ENDED_FMT="附加过程中会话已结束: %s"
_I18N_en_DEBUG_ATTACH_FAILED_FMT="Attach failed: session=%s status=%s rc=%s"
_I18N_zh_DEBUG_ATTACH_FAILED_FMT="附加失败: session=%s status=%s rc=%s"
_I18N_en_DEBUG_ATTACH_ENDED_FMT="Session ended during attach: %s (rc=%s)"
_I18N_zh_DEBUG_ATTACH_ENDED_FMT="附加过程中会话结束: %s (rc=%s)"

_I18N_en_ERR_SESSION_KILL_FAILED="Session close failed"
_I18N_zh_ERR_SESSION_KILL_FAILED="关闭会话失败"
_I18N_en_ERR_SESSION_KILL_TIMEOUT_FMT="Session still exists after close attempt: %s (max_attempts=%s, interval=%ss)"
_I18N_zh_ERR_SESSION_KILL_TIMEOUT_FMT="关闭后会话仍存在: %s (最大重试=%s, 间隔=%ss)"

_I18N_en_INFO_SESSION_KILLING="Closing session:"
_I18N_zh_INFO_SESSION_KILLING="关闭会话:"

_I18N_en_INFO_SESSION_CLOSED="Session closed:"
_I18N_zh_INFO_SESSION_CLOSED="会话已关闭:"

_I18N_en_ERR_SESSION_UNRESPONSIVE="Session unresponsive:"
_I18N_zh_ERR_SESSION_UNRESPONSIVE="会话无响应:"

_I18N_en_DEBUG_CONFIG_APPLIED="Config applied"
_I18N_zh_DEBUG_CONFIG_APPLIED="配置已应用"

_I18N_en_DEBUG_APPLYING_SESSION_CONFIG="Applying session config:"
_I18N_zh_DEBUG_APPLYING_SESSION_CONFIG="应用会话配置:"

_I18N_en_DEBUG_SESSION_HEALTHY="Session healthy:"
_I18N_zh_DEBUG_SESSION_HEALTHY="会话健康:"

_I18N_en_DEBUG_SEND_CMD_TO_PANE="Sending command to pane"
_I18N_zh_DEBUG_SEND_CMD_TO_PANE="发送命令到 pane"

# === layout_service ===

_I18N_en_ERR_BRANCH_DIR_EMPTY="Branch name and work directory cannot be empty"
_I18N_zh_ERR_BRANCH_DIR_EMPTY="分支名和工作目录不能为空"

_I18N_en_INFO_CREATING_LAYOUT="Creating layout:"
_I18N_zh_INFO_CREATING_LAYOUT="创建布局:"

_I18N_en_ERR_SPLIT_V_FAILED="Vertical split creation failed"
_I18N_zh_ERR_SPLIT_V_FAILED="创建垂直分割失败"

_I18N_en_ERR_SPLIT_RIGHT_FAILED="Right split creation failed"
_I18N_zh_ERR_SPLIT_RIGHT_FAILED="创建右侧分割失败"

_I18N_en_ERR_SPLIT_LEFT_H_FAILED="Left horizontal split creation failed"
_I18N_zh_ERR_SPLIT_LEFT_H_FAILED="创建左侧水平分割失败"

_I18N_en_ERR_SPLIT_H_FAILED="Horizontal split creation failed"
_I18N_zh_ERR_SPLIT_H_FAILED="创建水平分割失败"

_I18N_en_ERR_SPLIT_RIGHT_H_FAILED="Right horizontal split creation failed"
_I18N_zh_ERR_SPLIT_RIGHT_H_FAILED="创建右侧水平分割失败"

_I18N_en_ERR_UNKNOWN_LAYOUT="Unknown layout mode:"
_I18N_zh_ERR_UNKNOWN_LAYOUT="未知布局模式:"

_I18N_en_INFO_LAYOUT_CREATED="Layout created"
_I18N_zh_INFO_LAYOUT_CREATED="布局创建完成"

_I18N_en_INFO_APPLYING_LAYOUT_CMDS="Applying layout command config..."
_I18N_zh_INFO_APPLYING_LAYOUT_CMDS="应用布局命令配置..."

_I18N_en_INFO_LAYOUT_CMDS_APPLIED="Layout commands applied"
_I18N_zh_INFO_LAYOUT_CMDS_APPLIED="布局命令已应用"

_I18N_en_WARN_CMD_CONFIG_FAILED_WORKSPACE="Command config failed, but workspace created"
_I18N_zh_WARN_CMD_CONFIG_FAILED_WORKSPACE="应用命令配置失败,但工作空间已创建"

_I18N_en_INFO_WORKSPACE_READY="Workspace ready:"
_I18N_zh_INFO_WORKSPACE_READY="工作空间已就绪:"

_I18N_en_INFO_RELOADING_LAYOUT="Reloading layout config..."
_I18N_zh_INFO_RELOADING_LAYOUT="重新加载布局配置..."

_I18N_en_ERR_INVALID_DIRECTION="Invalid direction:"
_I18N_zh_ERR_INVALID_DIRECTION="无效的方向:"

_I18N_en_DEBUG_LAYOUT_MISMATCH="Layout mismatch: expected"
_I18N_zh_DEBUG_LAYOUT_MISMATCH="布局不匹配: 期望"

_I18N_en_DEBUG_LAYOUT_MISMATCH_FMT="Layout mismatch: expected %s panes, actual %s"
_I18N_zh_DEBUG_LAYOUT_MISMATCH_FMT="布局不匹配: 期望 %s 个 pane, 实际 %s 个"

_I18N_en_DEBUG_LAYOUT_COMPLETE="Layout complete:"
_I18N_zh_DEBUG_LAYOUT_COMPLETE="布局完整:"

_I18N_en_DEBUG_LAYOUT_COMPLETE_FMT="Layout complete: %s (%s panes)"
_I18N_zh_DEBUG_LAYOUT_COMPLETE_FMT="布局完整: %s (%s 个 pane)"

_I18N_en_DEBUG_LAYOUT_EXTRA_PANES_FMT="Extra panes detected: expected %s, actual %s (user-added)"
_I18N_zh_DEBUG_LAYOUT_EXTRA_PANES_FMT="检测到额外 pane: 期望 %s, 实际 %s (用户自定义)"

_I18N_en_DEBUG_LAYOUT_RESTORE_MISMATCH_FMT="Stale layout state: saved %s panes, current %s panes, skipping restore"
_I18N_zh_DEBUG_LAYOUT_RESTORE_MISMATCH_FMT="布局状态过期: 保存 %s 个 pane, 当前 %s 个 pane, 跳过恢复"

_I18N_en_DEBUG_FOCUS_MAIN_PANE="Focus main pane"
_I18N_zh_DEBUG_FOCUS_MAIN_PANE="聚焦到主 pane"

_I18N_en_WARN_ADAPTER_NO_RESIZE="Current adapter (%s) does not support precise pane resize"
_I18N_zh_WARN_ADAPTER_NO_RESIZE="当前适配器 (%s) 不支持精确的 pane 大小调整"

# === worktree_service (additional) ===

# === sync_files ===

_I18N_en_SYNC_FILES_START="Syncing files to worktree"
_I18N_zh_SYNC_FILES_START="同步文件到 worktree"

_I18N_en_SYNC_FILES_DONE="%s synced, %s skipped, %s failed"
_I18N_zh_SYNC_FILES_DONE="%s 已同步, %s 已跳过, %s 失败"

_I18N_en_SYNC_FILES_FAILED="File sync failed:"
_I18N_zh_SYNC_FILES_FAILED="文件同步失败:"

_I18N_en_SYNC_FILES_PATH_TRAVERSAL="Path traversal rejected:"
_I18N_zh_SYNC_FILES_PATH_TRAVERSAL="路径遍历已拒绝:"

_I18N_en_INFO_BRANCH_NOT_EXISTS_CREATING="Branch does not exist, creating from current branch:"
_I18N_zh_INFO_BRANCH_NOT_EXISTS_CREATING="分支不存在,将基于当前分支创建:"

_I18N_en_ERR_WT_CREATE_BRANCH_FAILED="Worktree and branch creation failed"
_I18N_zh_ERR_WT_CREATE_BRANCH_FAILED="创建 Worktree 和分支失败"

_I18N_en_ERR_WT_FORCE_REMOVE_FAILED="Force remove Worktree failed"
_I18N_zh_ERR_WT_FORCE_REMOVE_FAILED="强制删除 Worktree 失败"

_I18N_en_ERR_WT_REMOVE_UNCOMMITTED="Worktree removal failed (may have uncommitted changes)"
_I18N_zh_ERR_WT_REMOVE_UNCOMMITTED="删除 Worktree 失败 (可能有未提交的更改)"

_I18N_en_ERR_WT_CORRUPTED_GIT="Worktree corrupted: missing .git file"
_I18N_zh_ERR_WT_CORRUPTED_GIT="Worktree 损坏: 缺少 .git 文件"

_I18N_en_ERR_WT_CORRUPTED_BRANCH="Worktree corrupted: cannot get branch info"
_I18N_zh_ERR_WT_CORRUPTED_BRANCH="Worktree 损坏: 无法获取分支信息"

_I18N_en_INFO_WT_PRUNING="Pruning Worktree records..."
_I18N_zh_INFO_WT_PRUNING="清理 Worktree 记录..."

_I18N_en_INFO_WT_PRUNED="Worktree records pruned"
_I18N_zh_INFO_WT_PRUNED="Worktree 记录已清理"

# === pane command (open subcommand and shared messages) ===

_I18N_en_PANE_CONFIRM_CREATE_WT="Worktree '%s' does not exist. Create it?"
_I18N_zh_PANE_CONFIRM_CREATE_WT="Worktree '%s' 不存在。是否创建?"

_I18N_en_PANE_WT_CREATED="Worktree created:"
_I18N_zh_PANE_WT_CREATED="Worktree 已创建:"

_I18N_en_PANE_OPEN_DESC="Open a tool in the current pane for a Worktree"
_I18N_zh_PANE_OPEN_DESC="在当前 pane 中为 Worktree 打开工具"

_I18N_en_PANE_INFO_USE_PANE_OPEN="Use 'workhive pane open <worktree>' to open in current pane"
_I18N_zh_PANE_INFO_USE_PANE_OPEN="使用 'workhive pane open <worktree>' 在当前 pane 中打开"

_I18N_en_PANE_LAYOUT_TITLE="Switch Layout Template"
_I18N_zh_PANE_LAYOUT_TITLE="切换布局模板"

_I18N_en_PANE_LAYOUT_CURRENT="Current layout:"
_I18N_zh_PANE_LAYOUT_CURRENT="当前布局:"

_I18N_en_PANE_LAYOUT_SELECT="Select new layout"
_I18N_zh_PANE_LAYOUT_SELECT="选择新布局"

_I18N_en_PANE_LAYOUT_SAME="Already using this layout, no change needed"
_I18N_zh_PANE_LAYOUT_SAME="已在使用此布局，无需切换"

_I18N_en_PANE_LAYOUT_CONFIRM="Switch layout from '%s' to '%s'? (will rebuild all panes)"
_I18N_zh_PANE_LAYOUT_CONFIRM="从 '%s' 切换到 '%s'？(将重建所有 pane)"

_I18N_en_PANE_LAYOUT_CONFIRM_REBUILD="Apply new layout? (will rebuild all panes)"
_I18N_zh_PANE_LAYOUT_CONFIRM_REBUILD="应用新布局？（将重建所有窗格）"

_I18N_en_PANE_LAYOUT_SWITCHING="Switching layout to"
_I18N_zh_PANE_LAYOUT_SWITCHING="正在切换布局到"

_I18N_en_PANE_LAYOUT_REBUILDING="Rebuilding panes in current window with new layout..."
_I18N_zh_PANE_LAYOUT_REBUILDING="正在以新布局重建当前窗口的窗格..."

_I18N_en_PANE_LAYOUT_DONE="Layout rebuilt successfully"
_I18N_zh_PANE_LAYOUT_DONE="布局重建成功"

# === layout persistence ===

_I18N_en_LAYOUT_SAVED="Layout saved"
_I18N_zh_LAYOUT_SAVED="布局已保存"

_I18N_en_LAYOUT_RESTORED="Layout restored from saved state"
_I18N_zh_LAYOUT_RESTORED="布局已从保存状态恢复"

_I18N_en_LAYOUT_SAVE_FAILED="Failed to save layout"
_I18N_zh_LAYOUT_SAVE_FAILED="保存布局失败"

_I18N_en_MSG_PANE_CMDS_RESTORED="Previous pane commands restored"
_I18N_zh_MSG_PANE_CMDS_RESTORED="已恢复之前的 pane 命令"

# === upgrade command ===

_I18N_en_ERR_UPGRADE_NOT_GIT="Not a git repository, cannot upgrade via git pull"
_I18N_zh_ERR_UPGRADE_NOT_GIT="非 git 仓库，无法通过 git pull 升级"

_I18N_en_ERR_UPGRADE_VERSION_NOT_FOUND="Version not found:"
_I18N_zh_ERR_UPGRADE_VERSION_NOT_FOUND="版本不存在:"

_I18N_en_ERR_UPGRADE_PULL_FAILED="Git pull failed"
_I18N_zh_ERR_UPGRADE_PULL_FAILED="Git pull 失败"

_I18N_en_ERR_UPGRADE_DOWNLOAD_FAILED="Failed to download install script"
_I18N_zh_ERR_UPGRADE_DOWNLOAD_FAILED="下载安装脚本失败"

_I18N_en_ERR_UPGRADE_FAILED="Upgrade failed"
_I18N_zh_ERR_UPGRADE_FAILED="升级失败"

_I18N_en_ERR_MISSING_VERSION="Missing version argument"
_I18N_zh_ERR_MISSING_VERSION="缺少版本号参数"

_I18N_en_MSG_UPGRADE_CURRENT="Current version:"
_I18N_zh_MSG_UPGRADE_CURRENT="当前版本:"

_I18N_en_MSG_UPGRADE_CHECKING="Checking for updates"
_I18N_zh_MSG_UPGRADE_CHECKING="正在检查更新"

_I18N_en_MSG_UPGRADE_REMOTE="Latest version:"
_I18N_zh_MSG_UPGRADE_REMOTE="最新版本:"

_I18N_en_MSG_UPGRADE_UP_TO_DATE="Already up-to-date"
_I18N_zh_MSG_UPGRADE_UP_TO_DATE="已是最新版本"

_I18N_en_MSG_UPGRADE_AVAILABLE="Update available"
_I18N_zh_MSG_UPGRADE_AVAILABLE="有新版本可用"

_I18N_en_MSG_UPGRADE_RUN_CMD="Run the following to upgrade:"
_I18N_zh_MSG_UPGRADE_RUN_CMD="执行以下命令升级:"

_I18N_en_MSG_UPGRADE_WILL_UPDATE="Upgrade"
_I18N_zh_MSG_UPGRADE_WILL_UPDATE="升级"

_I18N_en_MSG_UPGRADE_CONFIRM="Proceed with upgrade?"
_I18N_zh_MSG_UPGRADE_CONFIRM="是否继续升级？"

_I18N_en_MSG_UPGRADE_COMPLETE="Upgrade complete"
_I18N_zh_MSG_UPGRADE_COMPLETE="升级完成"

_I18N_en_MSG_UPGRADE_GIT_PULL="Pulling latest changes from GitHub"
_I18N_zh_MSG_UPGRADE_GIT_PULL="正在从 GitHub 拉取最新代码"

_I18N_en_MSG_UPGRADE_REINSTALL="Reinstalling to"
_I18N_zh_MSG_UPGRADE_REINSTALL="正在重新安装到"

_I18N_en_MSG_UPGRADE_STASHED="Uncommitted changes stashed"
_I18N_zh_MSG_UPGRADE_STASHED="未提交的更改已暂存"

_I18N_en_MSG_UPGRADE_MANUAL="Check releases manually:"
_I18N_zh_MSG_UPGRADE_MANUAL="请手动查看版本:"

_I18N_en_MSG_UPGRADE_CONTINUE_ANYWAY="Continue anyway?"
_I18N_zh_MSG_UPGRADE_CONTINUE_ANYWAY="仍然继续？"

_I18N_en_WARN_UPGRADE_CANNOT_CHECK="Cannot check remote version (network issue?)"
_I18N_zh_WARN_UPGRADE_CANNOT_CHECK="无法检查远程版本 (网络问题?)"

_I18N_en_WARN_UPGRADE_UNCOMMITTED="You have uncommitted changes"
_I18N_zh_WARN_UPGRADE_UNCOMMITTED="你有未提交的更改"

_I18N_en_WARN_UPGRADE_FF_FAILED="Fast-forward merge failed, trying rebase..."
_I18N_zh_WARN_UPGRADE_FF_FAILED="快进合并失败, 尝试 rebase..."

_I18N_en_WARN_UPGRADE_DEV_MODE="Running in development mode (source not installed)"
_I18N_zh_WARN_UPGRADE_DEV_MODE="当前为开发模式 (未通过安装脚本安装)"

_I18N_en_MSG_UPGRADE_DEV_HINT="Please update manually:"
_I18N_zh_MSG_UPGRADE_DEV_HINT="请手动更新:"

_I18N_en_HELP_CMD_UPGRADE="Check for updates and upgrade"
_I18N_zh_HELP_CMD_UPGRADE="检查更新并升级"

_I18N_en_HELP_CMD_COMPLETIONS="output shell completion script"
_I18N_zh_HELP_CMD_COMPLETIONS="输出 Shell 补全脚本"

_I18N_en_MSG_ATTACH_NO_SESSION_AUTO_OPEN="No active session found for target:"
_I18N_zh_MSG_ATTACH_NO_SESSION_AUTO_OPEN="未找到目标活跃会话:"

_I18N_en_MSG_ATTACH_NO_SESSION_HAS_WT="No active sessions found"
_I18N_zh_MSG_ATTACH_NO_SESSION_HAS_WT="未发现活跃会话"

_I18N_en_MSG_ATTACH_SELECT_WT="Select session to attach"
_I18N_zh_MSG_ATTACH_SELECT_WT="选择要附加的会话"

# === list command dashboard ===

_I18N_en_STATUS_TITLE="Workspace Dashboard"
_I18N_zh_STATUS_TITLE="工作空间仪表盘"

_I18N_en_STATUS_SUMMARY="Sessions"
_I18N_zh_STATUS_SUMMARY="会话"

_I18N_en_STATUS_ACTIVE="active"
_I18N_zh_STATUS_ACTIVE="活跃"

_I18N_en_STATUS_DETACHED="detached"
_I18N_zh_STATUS_DETACHED="断开"
_I18N_en_STATUS_MIXED="mixed"
_I18N_zh_STATUS_MIXED="混合"

_I18N_en_STATUS_TOTAL="total"
_I18N_zh_STATUS_TOTAL="总计"

_I18N_en_STATUS_COL_GIT="Git Status"
_I18N_zh_STATUS_COL_GIT="Git 状态"

_I18N_en_STATUS_COL_ACTIVITY="Last Commit"
_I18N_zh_STATUS_COL_ACTIVITY="最近提交"

_I18N_en_STATUS_LEGEND="Legend:"
_I18N_zh_STATUS_LEGEND="图例:"

_I18N_en_STATUS_LEGEND_ACTIVE="active session"
_I18N_zh_STATUS_LEGEND_ACTIVE="活跃会话"

_I18N_en_STATUS_LEGEND_DETACHED="detached session"
_I18N_zh_STATUS_LEGEND_DETACHED="断开会话"

_I18N_en_STATUS_SESSION_DETAILS_TITLE="Session Details"
_I18N_zh_STATUS_SESSION_DETAILS_TITLE="会话详情"

_I18N_en_STATUS_SESSION_DETAILS_INLINE_LABEL="Session Details:"
_I18N_zh_STATUS_SESSION_DETAILS_INLINE_LABEL="会话明细:"

_I18N_en_STATUS_DETAIL_COL_STATE="State"
_I18N_zh_STATUS_DETAIL_COL_STATE="状态"

_I18N_en_STATUS_DETAIL_COL_RECENT_SESSION="Recent Session"
_I18N_zh_STATUS_DETAIL_COL_RECENT_SESSION="最近会话"

_I18N_en_STATUS_TIME_JUST_NOW="just now"
_I18N_zh_STATUS_TIME_JUST_NOW="刚刚"

_I18N_en_STATUS_TIME_MIN_AGO_FMT="%sm ago"
_I18N_zh_STATUS_TIME_MIN_AGO_FMT="%s分钟前"

_I18N_en_STATUS_TIME_HOUR_AGO_FMT="%sh ago"
_I18N_zh_STATUS_TIME_HOUR_AGO_FMT="%s小时前"

_I18N_en_STATUS_TIME_DAY_AGO_FMT="%sd ago"
_I18N_zh_STATUS_TIME_DAY_AGO_FMT="%s天前"

_I18N_en_STATUS_LAYOUT_TITLE="Layout Configuration"
_I18N_zh_STATUS_LAYOUT_TITLE="布局配置"

_I18N_en_STATUS_LAYOUT_MODE_LABEL="Mode:"
_I18N_zh_STATUS_LAYOUT_MODE_LABEL="模式:"

_I18N_en_STATUS_LAYOUT_SOURCE_LABEL="Source:"
_I18N_zh_STATUS_LAYOUT_SOURCE_LABEL="配置来源:"

_I18N_en_STATUS_LAYOUT_PANES_LABEL="Panes:"
_I18N_zh_STATUS_LAYOUT_PANES_LABEL="窗格数:"

_I18N_en_STATUS_LAYOUT_SRC_PROJECT="Project config"
_I18N_zh_STATUS_LAYOUT_SRC_PROJECT="项目配置"

_I18N_en_STATUS_LAYOUT_SRC_GLOBAL="Global config"
_I18N_zh_STATUS_LAYOUT_SRC_GLOBAL="全局配置"

_I18N_en_STATUS_LAYOUT_SRC_DEFAULT="Built-in default"
_I18N_zh_STATUS_LAYOUT_SRC_DEFAULT="内置默认值"

_I18N_en_STATUS_LAYOUT_PANE_CMDS="Pane Commands"
_I18N_zh_STATUS_LAYOUT_PANE_CMDS="窗格命令"

_I18N_en_STATUS_LAYOUT_VALIDATION="Session Layout Validation"
_I18N_zh_STATUS_LAYOUT_VALIDATION="会话布局验证"

_I18N_en_STATUS_LAYOUT_VALID="valid"
_I18N_zh_STATUS_LAYOUT_VALID="有效"

_I18N_en_STATUS_LAYOUT_INVALID="invalid (pane count mismatch)"
_I18N_zh_STATUS_LAYOUT_INVALID="无效 (窗格数不匹配)"

_I18N_en_STATUS_LAYOUT_EXPECTED="expected"
_I18N_zh_STATUS_LAYOUT_EXPECTED="期望"

_I18N_en_ERR_LAYOUT_MUTEX="--layout cannot be used with --format or --watch"
_I18N_zh_ERR_LAYOUT_MUTEX="--layout 不能与 --format 或 --watch 同时使用"
_I18N_en_ERR_WATCH_JSON_MUTEX="--watch cannot be used with --format json"
_I18N_zh_ERR_WATCH_JSON_MUTEX="--watch 不能与 --format json 同时使用"

_I18N_en_STATUS_ACTION_SWITCH="Switch workspace:"
_I18N_zh_STATUS_ACTION_SWITCH="切换工作空间:"

# === doctor command ===

_I18N_en_DOCTOR_TITLE="Environment Health Check"
_I18N_zh_DOCTOR_TITLE="环境健康检查"

_I18N_en_DOCTOR_CHECKING="Checking dependencies..."
_I18N_zh_DOCTOR_CHECKING="检查依赖..."

_I18N_en_DOCTOR_REQUIRED="Required"
_I18N_zh_DOCTOR_REQUIRED="必需"

_I18N_en_DOCTOR_OPTIONAL="Optional"
_I18N_zh_DOCTOR_OPTIONAL="可选"

_I18N_en_DOCTOR_RECOMMENDED="Recommended"
_I18N_zh_DOCTOR_RECOMMENDED="推荐"

_I18N_en_DOCTOR_INSTALLED="installed"
_I18N_zh_DOCTOR_INSTALLED="已安装"

_I18N_en_DOCTOR_MISSING="missing"
_I18N_zh_DOCTOR_MISSING="缺失"

_I18N_en_DOCTOR_VERSION="version"
_I18N_zh_DOCTOR_VERSION="版本"

_I18N_en_DOCTOR_CONFIG_CHECK="Checking configuration..."
_I18N_zh_DOCTOR_CONFIG_CHECK="检查配置..."

_I18N_en_DOCTOR_CONFIG_OK="Config file valid"
_I18N_zh_DOCTOR_CONFIG_OK="配置文件有效"

_I18N_en_DOCTOR_CONFIG_MISSING="Config file missing (run: workhive init)"
_I18N_zh_DOCTOR_CONFIG_MISSING="配置文件缺失 (运行: workhive init)"

_I18N_en_DOCTOR_CONFIG_INVALID="Config file has syntax errors"
_I18N_zh_DOCTOR_CONFIG_INVALID="配置文件有语法错误"

_I18N_en_DOCTOR_CONFIG_GLOBAL_ONLY="Global config only (no project config)"
_I18N_zh_DOCTOR_CONFIG_GLOBAL_ONLY="仅有全局配置 (无项目配置)"

_I18N_en_DOCTOR_TMUX_CHECK="Checking tmux..."
_I18N_zh_DOCTOR_TMUX_CHECK="检查 tmux..."

_I18N_en_DOCTOR_TMUX_VERSION_LOW="tmux version too low (recommend 3.3+)"
_I18N_zh_DOCTOR_TMUX_VERSION_LOW="tmux 版本过低 (推荐 3.3+)"

_I18N_en_DOCTOR_GIT_CHECK="Checking git..."
_I18N_zh_DOCTOR_GIT_CHECK="检查 git..."

_I18N_en_DOCTOR_NOT_GIT_REPO="Not in a git repository"
_I18N_zh_DOCTOR_NOT_GIT_REPO="不在 git 仓库中"

_I18N_en_DOCTOR_ALL_OK="All checks passed!"
_I18N_zh_DOCTOR_ALL_OK="所有检查通过!"

_I18N_en_DOCTOR_HAS_ISSUES="Issues found"
_I18N_zh_DOCTOR_HAS_ISSUES="发现问题"

_I18N_en_DOCTOR_INSTALL_HINT="Install with:"
_I18N_zh_DOCTOR_INSTALL_HINT="安装方法:"

# === gc command ===

_I18N_en_GC_TITLE="Garbage Collection"
_I18N_zh_GC_TITLE="垃圾回收"

_I18N_en_GC_SCANNING="Scanning for merged branches..."
_I18N_zh_GC_SCANNING="扫描已合并的分支..."

_I18N_en_GC_NO_MERGED="No merged branches found to clean up"
_I18N_zh_GC_NO_MERGED="没有可清理的已合并分支"

_I18N_en_GC_FOUND_MERGED="Found merged branches:"
_I18N_zh_GC_FOUND_MERGED="发现已合并的分支:"

_I18N_en_GC_CONFIRM="Remove these worktrees? (session + worktree + branch)"
_I18N_zh_GC_CONFIRM="删除这些 worktree？(会话 + worktree + 分支)"

_I18N_en_GC_REMOVING="Removing:"
_I18N_zh_GC_REMOVING="正在删除:"

_I18N_en_GC_COMPLETE="Garbage collection complete"
_I18N_zh_GC_COMPLETE="垃圾回收完成"

_I18N_en_GC_CLEANED="cleaned"
_I18N_zh_GC_CLEANED="已清理"

_I18N_en_GC_SKIPPED="Skipped (cancelled)"
_I18N_zh_GC_SKIPPED="已跳过 (取消)"

_I18N_en_GC_SCANNING_UNMODIFIED="Scanning for unmodified worktrees..."
_I18N_zh_GC_SCANNING_UNMODIFIED="扫描无修改的 Worktree..."

_I18N_en_GC_FOUND_UNMODIFIED="Unmodified worktrees (no commits, no changes):"
_I18N_zh_GC_FOUND_UNMODIFIED="无修改的 Worktree (无提交, 无更改):"

_I18N_en_GC_CONFIRM_UNMODIFIED="Remove these unmodified worktrees?"
_I18N_zh_GC_CONFIRM_UNMODIFIED="删除这些无修改的 Worktree？"

_I18N_en_GC_NO_UNMODIFIED="No unmodified worktrees found"
_I18N_zh_GC_NO_UNMODIFIED="没有无修改的 Worktree"

_I18N_en_GC_ORPHAN_SESSIONS="Orphan sessions (no worktree):"
_I18N_zh_GC_ORPHAN_SESSIONS="孤立会话 (无 worktree):"

_I18N_en_GC_CONFIRM_ORPHAN="Remove these orphan sessions?"
_I18N_zh_GC_CONFIRM_ORPHAN="删除这些孤立会话？"

_I18N_en_GC_STALE_STATE="Stale state files:"
_I18N_zh_GC_STALE_STATE="过期状态文件:"

_I18N_en_GC_CONFIRM_STALE="Remove these stale state files?"
_I18N_zh_GC_CONFIRM_STALE="删除这些过期状态文件？"

_I18N_en_GC_SCANNING_SNAPSHOTS="Scanning snapshot files..."
_I18N_zh_GC_SCANNING_SNAPSHOTS="扫描快照文件..."

_I18N_en_GC_NO_SNAPSHOTS="No snapshot files found"
_I18N_zh_GC_NO_SNAPSHOTS="没有快照文件"

_I18N_en_GC_FOUND_SNAPSHOTS="Snapshot files:"
_I18N_zh_GC_FOUND_SNAPSHOTS="快照文件:"

_I18N_en_GC_CONFIRM_SNAPSHOTS="Delete these snapshot files?"
_I18N_zh_GC_CONFIRM_SNAPSHOTS="删除这些快照文件？"

# === notes command ===

_I18N_en_NOTES_TITLE="Worktree Notes"
_I18N_zh_NOTES_TITLE="工作区笔记"

_I18N_en_NOTES_CREATED="Notes file created:"
_I18N_zh_NOTES_CREATED="笔记文件已创建:"

_I18N_en_NOTES_OPENING="Opening notes:"
_I18N_zh_NOTES_OPENING="打开笔记:"

_I18N_en_NOTES_NO_EDITOR="No editor found (set EDITOR env variable)"
_I18N_zh_NOTES_NO_EDITOR="未找到编辑器 (请设置 EDITOR 环境变量)"

# === custom layout ===

_I18N_en_CUSTOM_LAYOUT_PROMPT="Configure custom grid layout (rows x cols)"
_I18N_zh_CUSTOM_LAYOUT_PROMPT="配置自定义网格布局 (行 x 列)"

_I18N_en_CUSTOM_GRID_ROWS="Number of rows"
_I18N_zh_CUSTOM_GRID_ROWS="行数"

_I18N_en_CUSTOM_GRID_COLS="Number of columns"
_I18N_zh_CUSTOM_GRID_COLS="列数"

_I18N_en_CUSTOM_GRID_MAX_COLS="Max columns for %s rows (rows x cols ≤ 10)"
_I18N_zh_CUSTOM_GRID_MAX_COLS="%s 行时最多列数 (行 x 列 ≤ 10)"

_I18N_en_CUSTOM_INVALID_ROWS="Please enter a number between 1 and 10"
_I18N_zh_CUSTOM_INVALID_ROWS="请输入 1 到 10 之间的数字"

_I18N_en_CUSTOM_INVALID_COLS="Please enter a number between 1 and %s"
_I18N_zh_CUSTOM_INVALID_COLS="请输入 1 到 %s 之间的数字"

_I18N_en_CUSTOM_INVALID_COUNT="Please enter a number between 1 and 10"
_I18N_zh_CUSTOM_INVALID_COUNT="请输入 1 到 10 之间的数字"

_I18N_en_CUSTOM_CONFIGURE_CMDS="Configure command for each pane (required)"
_I18N_zh_CUSTOM_CONFIGURE_CMDS="配置每个 Pane 的命令 (必须)"

_I18N_en_CUSTOM_PANE_LABEL="Row %s Col %s"
_I18N_zh_CUSTOM_PANE_LABEL="行 %s 列 %s"

_I18N_en_MSG_LAYOUT_MODE_CUSTOM="custom (R x C grid, up to 10 panes)"
_I18N_zh_MSG_LAYOUT_MODE_CUSTOM="custom (R x C 网格, 最多 10 个 Pane)"

# === global config ===

_I18N_en_TITLE_GLOBAL_CONFIG="Global Configuration"
_I18N_zh_TITLE_GLOBAL_CONFIG="全局配置"

_I18N_en_INFO_NO_GLOBAL_CONFIG="No global config found."
_I18N_zh_INFO_NO_GLOBAL_CONFIG="未找到全局配置。"

_I18N_en_INFO_GLOBAL_CONFIG_FILE="Global config:"
_I18N_zh_INFO_GLOBAL_CONFIG_FILE="全局配置文件:"

_I18N_en_ASK_CREATE_GLOBAL_CONFIG="Global config not found. Create now via 'workhive init --global'?"
_I18N_zh_ASK_CREATE_GLOBAL_CONFIG="全局配置不存在。是否通过 'workhive init --global' 创建？"

_I18N_en_ASK_CREATE_PROJECT_CONFIG="Project config not found. Create now via 'workhive init'?"
_I18N_zh_ASK_CREATE_PROJECT_CONFIG="项目配置不存在。是否通过 'workhive init' 创建？"

_I18N_en_INFO_CREATED_GLOBAL_CONFIG="Global config created:"
_I18N_zh_INFO_CREATED_GLOBAL_CONFIG="全局配置已创建:"

_I18N_en_INFO_GLOBAL_CONFIG_PRIORITY="Priority: Project (.workhive/config.yaml) > Global > Built-in defaults"
_I18N_zh_INFO_GLOBAL_CONFIG_PRIORITY="优先级: 项目 (.workhive/config.yaml) > 全局 > 内置默认值"

_I18N_en_INFO_GLOBAL_TOOLS_TEMPLATE_INITIALIZED="Global tools template initialized."
_I18N_zh_INFO_GLOBAL_TOOLS_TEMPLATE_INITIALIZED="全局 tools 模板已初始化。"

_I18N_en_INFO_TOOLS_TEMPLATE_SCOPE="Tools are command templates for quick selection only; they do not overwrite manually entered pane commands."
_I18N_zh_INFO_TOOLS_TEMPLATE_SCOPE="tools 仅用于快速选择命令模板，不会覆盖你手动输入的 pane 命令。"

_I18N_en_ASK_USE_DEFAULT_TOOLS_TEMPLATE="Use default tools template now?"
_I18N_zh_ASK_USE_DEFAULT_TOOLS_TEMPLATE="现在使用默认 tools 模板吗?"

_I18N_en_INFO_OPENING_TOOLS_MANAGER="Opening tools manager..."
_I18N_zh_INFO_OPENING_TOOLS_MANAGER="正在打开 tools 管理器..."

_I18N_en_INFO_USE_FORCE_REINIT="Use --force to re-initialize"
_I18N_zh_INFO_USE_FORCE_REINIT="使用 --force 重新初始化"

_I18N_en_MSG_APPLYING_THEME="Applying tmux theme:"
_I18N_zh_MSG_APPLYING_THEME="应用 tmux 主题:"

_I18N_en_DEBUG_VERTICAL_SPLIT="Vertical split (right side"
_I18N_zh_DEBUG_VERTICAL_SPLIT="垂直分割 (右侧"

_I18N_en_DEBUG_HORIZONTAL_SPLIT_LEFT="Horizontal split left (bottom"
_I18N_zh_DEBUG_HORIZONTAL_SPLIT_LEFT="水平分割左侧 (下方"

_I18N_en_DEBUG_HORIZONTAL_SPLIT_RIGHT="Horizontal split right (bottom"
_I18N_zh_DEBUG_HORIZONTAL_SPLIT_RIGHT="水平分割右侧 (下方"

_I18N_en_DEBUG_SYNCING_WORKTREE="Syncing Git Worktree state..."
_I18N_zh_DEBUG_SYNCING_WORKTREE="正在同步 Git Worktree 状态..."

_I18N_en_DEBUG_SPLIT_RIGHT_AREA="Split right area"
_I18N_zh_DEBUG_SPLIT_RIGHT_AREA="分割右侧区域"

_I18N_en_DEBUG_SPLIT_LEFT_TOP_BOTTOM="Split left top/bottom"
_I18N_zh_DEBUG_SPLIT_LEFT_TOP_BOTTOM="分割左侧上下区域"

_I18N_en_DEBUG_SPLIT_RIGHT_TOP_BOTTOM="Split right top/bottom"
_I18N_zh_DEBUG_SPLIT_RIGHT_TOP_BOTTOM="分割右侧上下区域"

_I18N_en_DEBUG_CUSTOM_LAYOUT_LR="Custom grid layout: %s rows x %s cols, creating grid"
_I18N_zh_DEBUG_CUSTOM_LAYOUT_LR="自定义网格布局: %s 行 x %s 列, 创建网格"

_I18N_en_DEBUG_CUSTOM_LAYOUT_LEFT_ONLY="Custom grid layout: %s rows x 1 col (single column)"
_I18N_zh_DEBUG_CUSTOM_LAYOUT_LEFT_ONLY="自定义网格布局: %s 行 x 1 列 (单列)"

_I18N_en_DEBUG_LEFT_SPLIT_N="Left split #%s (new pane %s%%)"
_I18N_zh_DEBUG_LEFT_SPLIT_N="左侧分割第 %s 次 (新 pane %s%%)"

_I18N_en_DEBUG_RIGHT_SPLIT_N="Right split #%s (new pane %s%%)"
_I18N_zh_DEBUG_RIGHT_SPLIT_N="右侧分割第 %s 次 (新 pane %s%%)"

_I18N_en_DEBUG_WORKTREE_HEALTHY="Worktree healthy:"
_I18N_zh_DEBUG_WORKTREE_HEALTHY="Worktree 健康:"

_I18N_en_DEBUG_DELETED_WORKTREE="Deleted Worktree:"
_I18N_zh_DEBUG_DELETED_WORKTREE="已删除 Worktree:"

_I18N_en_WARN_WORKTREE_DELETE_FAILED="Worktree delete failed (trying force cleanup):"
_I18N_zh_WARN_WORKTREE_DELETE_FAILED="Worktree 删除失败 (尝试强制清理):"

_I18N_en_DEBUG_BRANCH_NOT_EXISTS="Branch no longer exists:"
_I18N_zh_DEBUG_BRANCH_NOT_EXISTS="分支已不存在:"

_I18N_en_DEBUG_CONFIG_FILE_NOT_EXISTS="Config file not found:"
_I18N_zh_DEBUG_CONFIG_FILE_NOT_EXISTS="配置文件不存在:"

_I18N_en_DEBUG_CONFIG_UPDATED="Config updated:"
_I18N_zh_DEBUG_CONFIG_UPDATED="配置已更新:"

_I18N_en_MSG_OPEN_INTERRUPTED="Operation interrupted, rolling back..."
_I18N_zh_MSG_OPEN_INTERRUPTED="操作中断，正在回滚..."

_I18N_en_MSG_ROLLBACK_SESSION="Rolling back: removing session"
_I18N_zh_MSG_ROLLBACK_SESSION="回滚: 清理会话"

_I18N_en_MSG_ROLLBACK_WORKTREE="Rolling back: removing worktree"
_I18N_zh_MSG_ROLLBACK_WORKTREE="回滚: 清理 worktree"

_I18N_en_ERR_YQ_VERSION_INCOMPATIBLE="Incompatible yq version detected. workhive requires mikefarah/yq v4+"
_I18N_zh_ERR_YQ_VERSION_INCOMPATIBLE="检测到不兼容的 yq 版本。workhive 需要 mikefarah/yq v4+"
_I18N_en_ERR_YQ_IMPL_INCOMPATIBLE="Incompatible yq implementation detected. workhive requires mikefarah/yq (Go implementation)"
_I18N_zh_ERR_YQ_IMPL_INCOMPATIBLE="检测到不兼容的 yq 实现。workhive 需要 mikefarah/yq（Go 实现）"
_I18N_en_ERR_YQ_VERSION_PIN_MISMATCH_FMT="yq version mismatch: required %s, detected %s"
_I18N_zh_ERR_YQ_VERSION_PIN_MISMATCH_FMT="yq 版本不匹配：要求 %s，检测到 %s"

_I18N_en_ERR_INVALID_PANE_ROLE="Invalid pane role:"
_I18N_zh_ERR_INVALID_PANE_ROLE="无效的 pane 角色名:"

_I18N_en_ERR_CONFIG_NOT_READABLE="Config file not readable (check permissions):"
_I18N_zh_ERR_CONFIG_NOT_READABLE="配置文件不可读 (请检查权限):"

# === destroy command (dirty worktree marker) ===

_I18N_en_WARN_WT_HAS_UNCOMMITTED="has uncommitted changes"
_I18N_zh_WARN_WT_HAS_UNCOMMITTED="有未提交更改"

_I18N_en_WARN_WT_HAS_UNMERGED="has unmerged commits"
_I18N_zh_WARN_WT_HAS_UNMERGED="有未合并提交"

_I18N_en_WARN_UNCOMMITTED_OR_UNMERGED_ALERT="WARNING: UNCOMMITTED CHANGES OR UNMERGED COMMITS DETECTED!"
_I18N_zh_WARN_UNCOMMITTED_OR_UNMERGED_ALERT="警告：检测到未提交的更改或未合并的提交！"

_I18N_en_WARN_UNMERGED_COMMITS="unmerged commits to main"
_I18N_zh_WARN_UNMERGED_COMMITS="个未合并到主分支的提交"

# === destroy/gc uncommitted changes alert ===

_I18N_en_WARN_UNCOMMITTED_ALERT="WARNING: UNCOMMITTED CHANGES DETECTED!"
_I18N_zh_WARN_UNCOMMITTED_ALERT="警告：检测到未提交的更改！"

_I18N_en_WARN_UNCOMMITTED_LOST="These changes will be PERMANENTLY LOST!"
_I18N_zh_WARN_UNCOMMITTED_LOST="这些更改将被永久丢失，无法恢复！"

_I18N_en_CONFIRM_DESTROY_DIRTY_TYPED="I understand uncommitted changes will be lost, continue?"
_I18N_zh_CONFIRM_DESTROY_DIRTY_TYPED="我了解未提交的更改将丢失，确认继续?"

_I18N_en_GC_WARN_DIRTY_BRANCHES="The following branches have uncommitted changes:"
_I18N_zh_GC_WARN_DIRTY_BRANCHES="以下分支有未提交的更改："

_I18N_en_GC_CONFIRM_DIRTY="These branches have uncommitted changes that will be lost. Continue?"
_I18N_zh_GC_CONFIRM_DIRTY="这些分支有未提交的更改将被丢失，确认继续?"

_I18N_en_GC_SKIPPED_DIRTY="Skipped branches with uncommitted changes"
_I18N_zh_GC_SKIPPED_DIRTY="已跳过有未提交更改的分支"


_I18N_en_RENAME_CONFIRM="Rename this workspace? (branch, worktree, and session state will change)"
_I18N_zh_RENAME_CONFIRM="确认重命名该工作空间？（branch、worktree 和会话状态都会变更）"

_I18N_en_CONFIRM_REMOVE_WT="Confirm remove this workspace? (worktree and branch will be deleted)"
_I18N_zh_CONFIRM_REMOVE_WT="确认删除该工作空间？（worktree 和分支都会被删除）"

_I18N_en_GC_HINT_MERGED_FMT="💡 %d merged branch(es) detected. Run 'workhive gc' to clean up."
_I18N_zh_GC_HINT_MERGED_FMT="💡 检测到 %d 个已合并的分支。运行 'workhive gc' 进行清理。"

_I18N_en_RENAME_ERR_SPACES="Branch name cannot contain spaces:"
_I18N_zh_RENAME_ERR_SPACES="分支名不能包含空格:"

_I18N_en_RENAME_ERR_DOTDOT="Branch name contains illegal characters:"
_I18N_zh_RENAME_ERR_DOTDOT="分支名包含非法字符:"

_I18N_en_RENAME_ERR_LEADING_HYPHEN="Branch name cannot start with hyphen:"
_I18N_zh_RENAME_ERR_LEADING_HYPHEN="分支名不能以连字符开头:"

_I18N_en_RENAME_ERR_INVALID_CHARS="Branch name contains invalid characters:"
_I18N_zh_RENAME_ERR_INVALID_CHARS="分支名包含无效字符:"

_I18N_en_RENAME_ERR_SAME="Old and new branch names are identical."
_I18N_zh_RENAME_ERR_SAME="新旧分支名相同。"

_I18N_en_RENAME_ERR_MAIN_FMT="Cannot rename the main branch (%s)."
_I18N_zh_RENAME_ERR_MAIN_FMT="不能重命名主分支 (%s)。"

_I18N_en_RENAME_ERR_NEW_EXISTS_FMT="Branch '%s' already exists."
_I18N_zh_RENAME_ERR_NEW_EXISTS_FMT="分支 '%s' 已存在。"

_I18N_en_RENAME_ERR_WT_NEW_EXISTS_FMT="Worktree for '%s' already exists."
_I18N_zh_RENAME_ERR_WT_NEW_EXISTS_FMT="'%s' 的 Worktree 已存在。"

_I18N_en_RENAME_ERR_WT_OLD_MISSING_FMT="Worktree for '%s' does not exist."
_I18N_zh_RENAME_ERR_WT_OLD_MISSING_FMT="'%s' 的 Worktree 不存在。"

_I18N_en_RENAME_ERR_ATTACHED_FMT="Session '%s' is attached. Use --force to rename anyway."
_I18N_zh_RENAME_ERR_ATTACHED_FMT="会话 '%s' 已附加。使用 --force 强制重命名。"

_I18N_en_RENAME_ERR_BOTH_REQUIRED="Both old_branch and new_branch are required."
_I18N_zh_RENAME_ERR_BOTH_REQUIRED="旧分支名和新分支名都是必需的。"

_I18N_en_RENAME_ERR_TOO_MANY="Too many arguments. Usage: workhive rename <old_branch> <new_branch>"
_I18N_zh_RENAME_ERR_TOO_MANY="参数过多。用法: workhive rename <旧分支> <新分支>"

_I18N_en_RENAME_STEP_BRANCH="Step 1/4: Renaming Git branch..."
_I18N_zh_RENAME_STEP_BRANCH="步骤 1/4: 重命名 Git 分支..."

_I18N_en_RENAME_STEP_WORKTREE="Step 2/4: Moving worktree..."
_I18N_zh_RENAME_STEP_WORKTREE="步骤 2/4: 移动 worktree..."

_I18N_en_RENAME_STEP_SESSION="Step 3/4: Renaming session..."
_I18N_zh_RENAME_STEP_SESSION="步骤 3/4: 重命名会话..."

_I18N_en_RENAME_STEP_STATE="Step 4/4: Renaming state files..."
_I18N_zh_RENAME_STEP_STATE="步骤 4/4: 重命名状态文件..."

_I18N_en_RENAME_ERR_BRANCH_FAIL="Failed to rename Git branch."
_I18N_zh_RENAME_ERR_BRANCH_FAIL="Git 分支重命名失败。"

_I18N_en_RENAME_ERR_WT_FAIL="Failed to move worktree."
_I18N_zh_RENAME_ERR_WT_FAIL="Worktree 移动失败。"

_I18N_en_RENAME_ERR_SESSION_FAIL="Failed to rename session."
_I18N_zh_RENAME_ERR_SESSION_FAIL="会话重命名失败。"

_I18N_en_RENAME_NO_SESSION="No session to rename (session not running)."
_I18N_zh_RENAME_NO_SESSION="无需重命名会话 (会话未运行)。"

_I18N_en_RENAME_TITLE_FMT="Rename workspace: %s → %s"
_I18N_zh_RENAME_TITLE_FMT="重命名工作空间: %s → %s"

_I18N_en_RENAME_SUCCESS_FMT="Workspace renamed: %s → %s"
_I18N_zh_RENAME_SUCCESS_FMT="工作空间已重命名: %s → %s"

_I18N_en_RENAME_ROLLBACK="Rename interrupted, rolling back..."
_I18N_zh_RENAME_ROLLBACK="重命名被中断，正在回滚..."

_I18N_en_RENAME_ERR_INVALID_REF="Branch name is not a valid git ref:"
_I18N_zh_RENAME_ERR_INVALID_REF="分支名不是有效的 git 引用:"

_I18N_en_STATUS_INTERVAL_NAN="--interval value must be a positive number"
_I18N_zh_STATUS_INTERVAL_NAN="--interval 的值必须是正数"

# === config set/get ===

_I18N_en_ERR_CONFIG_SET_USAGE="Usage: workhive config set <key> <value> [--global]"
_I18N_zh_ERR_CONFIG_SET_USAGE="用法: workhive config set <key> <value> [--global]"

_I18N_en_ERR_CONFIG_SET_TOO_MANY="Too many arguments for config set"
_I18N_zh_ERR_CONFIG_SET_TOO_MANY="config set 参数过多"

_I18N_en_ERR_CONFIG_GET_USAGE="Usage: workhive config get <key> [--global]"
_I18N_zh_ERR_CONFIG_GET_USAGE="用法: workhive config get <key> [--global]"

_I18N_en_ERR_CONFIG_GET_TOO_MANY="Too many arguments for config get"
_I18N_zh_ERR_CONFIG_GET_TOO_MANY="config get 参数过多"

_I18N_en_MSG_CONFIG_SET_OK="Config updated:"
_I18N_zh_MSG_CONFIG_SET_OK="配置已更新:"

# === close command ===

_I18N_en_CLOSE_NOT_IN_TMUX="close can only be used inside a tmux pane (i.e. inside a workhive workspace)"
_I18N_zh_CLOSE_NOT_IN_TMUX="close 只能在 tmux pane 内使用 (即在 workhive 工作空间内部)"

_I18N_en_CLOSE_CONFIRM="Close workspace '%s' and return to normal terminal?"
_I18N_zh_CLOSE_CONFIRM="关闭工作空间 '%s' 并返回到普通终端？"

_I18N_en_CLOSE_CLOSING="Closing workspace:"
_I18N_zh_CLOSE_CLOSING="正在关闭工作空间:"

_I18N_en_CLOSE_SUCCESS="Workspace closed:"
_I18N_zh_CLOSE_SUCCESS="工作空间已关闭:"

_I18N_en_CLOSE_CANCELLED="Close cancelled"
_I18N_zh_CLOSE_CANCELLED="已取消关闭"

_I18N_en_CLOSE_NOT_WORKHIVE_SESSION="Current session is not a workhive-managed session"
_I18N_zh_CLOSE_NOT_WORKHIVE_SESSION="当前会话不是 workhive 管理的会话"

_I18N_en_CLOSE_SELECT_ACTION="Select close action"
_I18N_zh_CLOSE_SELECT_ACTION="选择关闭方式"

_I18N_en_CLOSE_OPT_DETACH="Detach only (keep session alive, recommended)"
_I18N_zh_CLOSE_OPT_DETACH="仅分离 (保留会话, 推荐)"

_I18N_en_CLOSE_OPT_KILL="Terminate session (destroy all panes)"
_I18N_zh_CLOSE_OPT_KILL="终止会话 (销毁所有窗格)"

_I18N_en_CLOSE_DETACHING="Detaching from workspace:"
_I18N_zh_CLOSE_DETACHING="正在从工作空间分离:"

_I18N_en_CLOSE_DETACH_SUCCESS="Detached from workspace:"
_I18N_zh_CLOSE_DETACH_SUCCESS="已从工作空间分离:"

_I18N_en_CLOSE_DETACH_HINT="Session preserved. Re-enter with: workhive attach %s"
_I18N_zh_CLOSE_DETACH_HINT="会话已保留。重新进入: workhive attach %s"

_I18N_en_HELP_CMD_CLOSE="close current workspace and return to terminal"
_I18N_zh_HELP_CMD_CLOSE="关闭当前工作空间并返回终端"

# === tools registry keys ===

_I18N_en_TOOLS_REGISTRY_EMPTY="No tools found in registry"
_I18N_zh_TOOLS_REGISTRY_EMPTY="工具注册表中未找到任何工具"

_I18N_en_TOOLS_REGISTRY_FALLBACK="Using built-in tool detection (tools registry unavailable)"
_I18N_zh_TOOLS_REGISTRY_FALLBACK="使用内置工具检测 (工具注册表不可用)"

_I18N_en_TOOLS_NO_CONFIGURED="No tools configured."
_I18N_zh_TOOLS_NO_CONFIGURED="未配置任何工具。"

_I18N_en_TOOLS_COL_TOOL_NAME="tool_name"
_I18N_zh_TOOLS_COL_TOOL_NAME="工具名称"

_I18N_en_TOOLS_COL_LABEL="label"
_I18N_zh_TOOLS_COL_LABEL="标签"

_I18N_en_TOOLS_COL_AI="ai"
_I18N_zh_TOOLS_COL_AI="AI"

_I18N_en_TOOLS_COL_DEFAULT="default"
_I18N_zh_TOOLS_COL_DEFAULT="默认"

_I18N_en_TOOLS_COL_COMMAND="command"
_I18N_zh_TOOLS_COL_COMMAND="命令"

_I18N_en_TOOLS_ERR_INTERNAL_ID_CONFLICT_FMT="tool_name internal id conflict: '%s' conflicts with '%s' (normalized id: '%s')"
_I18N_zh_TOOLS_ERR_INTERNAL_ID_CONFLICT_FMT="tool_name 内部 id 冲突: '%s' 与 '%s' 冲突 (标准化 id: '%s')"

_I18N_en_TOOLS_ERR_ADD_REQUIRED="Required: --name <tool_name> --cmd <command>"
_I18N_zh_TOOLS_ERR_ADD_REQUIRED="必填: --name <tool_name> --cmd <command>"

_I18N_en_TOOLS_ERR_NAME_EXISTS_FMT="tool_name already exists: %s"
_I18N_zh_TOOLS_ERR_NAME_EXISTS_FMT="tool_name 已存在: %s"

_I18N_en_TOOLS_ERR_MISSING_NAME="Missing tool_name"
_I18N_zh_TOOLS_ERR_MISSING_NAME="缺少 tool_name"

_I18N_en_TOOLS_ERR_NAME_NOT_FOUND_FMT="tool_name not found: %s"
_I18N_zh_TOOLS_ERR_NAME_NOT_FOUND_FMT="tool_name 不存在: %s"

_I18N_en_TOOLS_ERR_INVALID_BOOL_FMT="Invalid boolean value for %s: %s (use true or false)"
_I18N_zh_TOOLS_ERR_INVALID_BOOL_FMT="%s 的布尔值无效: %s (请使用 true 或 false)"

_I18N_en_TOOLS_MSG_ADDED_FMT="Tool added: %s"
_I18N_zh_TOOLS_MSG_ADDED_FMT="工具已添加: %s"

_I18N_en_TOOLS_MSG_UPDATED_FMT="Tool updated: %s"
_I18N_zh_TOOLS_MSG_UPDATED_FMT="工具已更新: %s"

_I18N_en_TOOLS_MSG_REMOVED_FMT="Tool removed: %s"
_I18N_zh_TOOLS_MSG_REMOVED_FMT="工具已移除: %s"

_I18N_en_TOOLS_MSG_DEFAULT_SET_FMT="Default tool set: %s"
_I18N_zh_TOOLS_MSG_DEFAULT_SET_FMT="默认工具已设置: %s"

_I18N_en_TOOLS_MSG_VALID="Tools config is valid."
_I18N_zh_TOOLS_MSG_VALID="tools 配置有效。"

_I18N_en_TOOLS_ERR_TTY_REQUIRED="Interactive tools manager requires a TTY."
_I18N_zh_TOOLS_ERR_TTY_REQUIRED="交互式 tools 管理器需要 TTY 环境。"

_I18N_en_TOOLS_HINT_NON_INTERACTIVE="Use: workhive tools list|add|edit|remove|set-default|validate"
_I18N_zh_TOOLS_HINT_NON_INTERACTIVE="请使用: workhive tools list|add|edit|remove|set-default|validate"

_I18N_en_TOOLS_USAGE_LIST="Usage: workhive tools list"
_I18N_zh_TOOLS_USAGE_LIST="用法: workhive tools list"

_I18N_en_TOOLS_USAGE_VALIDATE="Usage: workhive tools validate"
_I18N_zh_TOOLS_USAGE_VALIDATE="用法: workhive tools validate"

_I18N_en_TOOLS_USAGE_MANAGE="Usage: workhive tools manage"
_I18N_zh_TOOLS_USAGE_MANAGE="用法: workhive tools manage"

_I18N_en_TOOLS_MANAGE_HELP_PURPOSE="Purpose: Open an interactive tools manager for global tools config."
_I18N_zh_TOOLS_MANAGE_HELP_PURPOSE="用途: 打开全局 tools 配置的交互式管理器。"

_I18N_en_TOOLS_MANAGE_HELP_ACTIONS="Available actions:"
_I18N_zh_TOOLS_MANAGE_HELP_ACTIONS="可执行操作:"

_I18N_en_TOOLS_MANAGE_HELP_NOTE="Use Enter to select an action; only choosing 'Done' exits manager."
_I18N_zh_TOOLS_MANAGE_HELP_NOTE="按 Enter 选择操作；只有选择“完成”才会退出管理器。"

_I18N_en_TOOLS_MENU_TITLE="Tools Manager"
_I18N_zh_TOOLS_MENU_TITLE="Tools 管理器"

_I18N_en_TOOLS_MENU_LIST="List tools"
_I18N_zh_TOOLS_MENU_LIST="列出工具"

_I18N_en_TOOLS_MENU_ADD="Add tool"
_I18N_zh_TOOLS_MENU_ADD="添加工具"

_I18N_en_TOOLS_MENU_EDIT="Edit tool"
_I18N_zh_TOOLS_MENU_EDIT="编辑工具"

_I18N_en_TOOLS_MENU_REMOVE="Remove tool"
_I18N_zh_TOOLS_MENU_REMOVE="删除工具"

_I18N_en_TOOLS_MENU_SET_DEFAULT="Set default tool"
_I18N_zh_TOOLS_MENU_SET_DEFAULT="设置默认工具"

_I18N_en_TOOLS_MENU_VALIDATE="Validate config"
_I18N_zh_TOOLS_MENU_VALIDATE="校验配置"

_I18N_en_TOOLS_MENU_DONE="Done"
_I18N_zh_TOOLS_MENU_DONE="完成"

_I18N_en_TOOLS_PROMPT_NAME="tool_name:"
_I18N_zh_TOOLS_PROMPT_NAME="工具名称(tool_name):"

_I18N_en_TOOLS_PROMPT_LABEL_FMT="label [%s]:"
_I18N_zh_TOOLS_PROMPT_LABEL_FMT="标签 [%s]:"

_I18N_en_TOOLS_PROMPT_COMMAND="command:"
_I18N_zh_TOOLS_PROMPT_COMMAND="命令:"

_I18N_en_TOOLS_PROMPT_IS_AI="is_ai (true/false) [false]:"
_I18N_zh_TOOLS_PROMPT_IS_AI="AI (true/false) [false]:"
_I18N_en_TOOLS_PROMPT_IS_AI_TUI="AI:"
_I18N_zh_TOOLS_PROMPT_IS_AI_TUI="AI:"

_I18N_en_TOOLS_PROMPT_DEFAULT="default (true/false) [false]:"
_I18N_zh_TOOLS_PROMPT_DEFAULT="默认 (true/false) [false，若 true 将覆盖其他默认]:"
_I18N_en_TOOLS_PROMPT_DEFAULT_TUI="Default (only one default tool):"
_I18N_zh_TOOLS_PROMPT_DEFAULT_TUI="默认 (仅能有一个默认工具):"

_I18N_en_TOOLS_PROMPT_ENABLED="enabled (true/false) [true]:"
_I18N_zh_TOOLS_PROMPT_ENABLED="启用 (true/false) [true]:"
_I18N_en_TOOLS_PROMPT_ENABLED_TUI="Enabled:"
_I18N_zh_TOOLS_PROMPT_ENABLED_TUI="启用:"

_I18N_en_TOOLS_PROMPT_EDIT_NAME="tool_name to edit:"
_I18N_zh_TOOLS_PROMPT_EDIT_NAME="待编辑的工具名称(tool_name):"

_I18N_en_TOOLS_PROMPT_NEW_LABEL="label (edit current value):"
_I18N_zh_TOOLS_PROMPT_NEW_LABEL="标签 (可直接编辑当前值):"

_I18N_en_TOOLS_PROMPT_NEW_COMMAND="command (edit current value):"
_I18N_zh_TOOLS_PROMPT_NEW_COMMAND="命令 (可直接编辑当前值):"

_I18N_en_TOOLS_PROMPT_NEW_IS_AI="new is_ai true/false (empty to skip):"
_I18N_zh_TOOLS_PROMPT_NEW_IS_AI="新 AI true/false (留空跳过):"
_I18N_en_TOOLS_PROMPT_NEW_IS_AI_TUI="AI (or skip):"
_I18N_zh_TOOLS_PROMPT_NEW_IS_AI_TUI="AI (或跳过):"

_I18N_en_TOOLS_PROMPT_NEW_ENABLED="new enabled true/false (empty to skip):"
_I18N_zh_TOOLS_PROMPT_NEW_ENABLED="新启用状态 true/false (留空跳过):"
_I18N_en_TOOLS_PROMPT_NEW_ENABLED_TUI="Enabled (or skip):"
_I18N_zh_TOOLS_PROMPT_NEW_ENABLED_TUI="启用 (或跳过):"

_I18N_en_TOOLS_PROMPT_SET_DEFAULT="set default true/false (empty to skip):"
_I18N_zh_TOOLS_PROMPT_SET_DEFAULT="设置默认 true/false (留空跳过，true 会覆盖其他默认):"
_I18N_en_TOOLS_PROMPT_SET_DEFAULT_TUI="Set default (or skip, true overrides previous default):"
_I18N_zh_TOOLS_PROMPT_SET_DEFAULT_TUI="设置默认 (或跳过，true 会覆盖旧默认):"

_I18N_en_TOOLS_BOOL_TRUE="true"
_I18N_zh_TOOLS_BOOL_TRUE="true"

_I18N_en_TOOLS_BOOL_FALSE="false"
_I18N_zh_TOOLS_BOOL_FALSE="false"

_I18N_en_TOOLS_BOOL_SKIP="skip"
_I18N_zh_TOOLS_BOOL_SKIP="跳过"

_I18N_en_TOOLS_PROMPT_REMOVE_NAME="tool_name to remove:"
_I18N_zh_TOOLS_PROMPT_REMOVE_NAME="待删除的工具名称(tool_name):"

_I18N_en_TOOLS_PROMPT_DEFAULT_NAME="tool_name to set default:"
_I18N_zh_TOOLS_PROMPT_DEFAULT_NAME="待设为默认的工具名称(tool_name):"

_I18N_en_TOOLS_ERR_UNKNOWN_SUBCMD="Unknown tools subcommand:"
_I18N_zh_TOOLS_ERR_UNKNOWN_SUBCMD="未知 tools 子命令:"

_I18N_en_TOOLS_ERR_COMMAND_EMPTY_FMT="Invalid tools config: command is required for enabled tool '%s'"
_I18N_zh_TOOLS_ERR_COMMAND_EMPTY_FMT="tools 配置无效: 已启用工具 '%s' 缺少 command"

_I18N_en_TOOLS_ERR_COMMAND_NOT_FOUND_FMT="Enabled tool '%s' command not found: %s (configured: %s)"
_I18N_zh_TOOLS_ERR_COMMAND_NOT_FOUND_FMT="已启用工具 '%s' 的命令不存在: %s (配置值: %s)"

# === builtin persistence ===

_I18N_en_PERSIST_ERR_NO_PREFIX="Cannot determine session prefix for snapshot save"
_I18N_zh_PERSIST_ERR_NO_PREFIX="无法确定 session 前缀，跳过快照保存"

_I18N_en_PERSIST_WARN_INVALID_SNAPSHOT="Snapshot validation failed, skipping restore"
_I18N_zh_PERSIST_WARN_INVALID_SNAPSHOT="快照验证失败，跳过恢复"

_I18N_en_PERSIST_RESTORING="Restoring sessions from snapshot..."
_I18N_zh_PERSIST_RESTORING="正在从快照恢复会话..."

_I18N_en_PERSIST_RESTORED="Sessions restored from snapshot"
_I18N_zh_PERSIST_RESTORED="会话已从快照恢复"

_I18N_en_PERSIST_RESTORING_SESSION="Restoring session from snapshot: %s"
_I18N_zh_PERSIST_RESTORING_SESSION="正在从快照恢复会话: %s"

_I18N_en_PERSIST_SESSION_RESTORED="Session restored from snapshot: %s"
_I18N_zh_PERSIST_SESSION_RESTORED="会话已从快照恢复: %s"

_I18N_en_PERSIST_RESTORED_FROM_SNAPSHOT="Session restored from snapshot (layout + pane commands preserved)"
_I18N_zh_PERSIST_RESTORED_FROM_SNAPSHOT="会话已从快照恢复 (布局 + pane 命令已保留)"

_I18N_en_PERSIST_TITLE="Session Persistence"
_I18N_zh_PERSIST_TITLE="会话持久化"

_I18N_en_PERSIST_LAYOUT_SKIP_SNAPSHOT="Layout restored from snapshot, skipping config-based layout creation"
_I18N_zh_PERSIST_LAYOUT_SKIP_SNAPSHOT="布局已从快照恢复，跳过配置文件布局创建"

_I18N_en_PERSIST_SAVED="Snapshot saved"
_I18N_zh_PERSIST_SAVED="快照已保存"

_I18N_en_PERSIST_SAVE_FAILED="Snapshot save failed"
_I18N_zh_PERSIST_SAVE_FAILED="快照保存失败"

_I18N_en_PERSIST_STATUS_LABEL="Status:"
_I18N_zh_PERSIST_STATUS_LABEL="状态:"

_I18N_en_PERSIST_AUTOSAVE_LABEL="Auto-save:"
_I18N_zh_PERSIST_AUTOSAVE_LABEL="自动保存:"

_I18N_en_PERSIST_AUTORESTORE_LABEL="Auto-restore:"
_I18N_zh_PERSIST_AUTORESTORE_LABEL="自动恢复:"

_I18N_en_PERSIST_LAST_BACKUP_LABEL="Last backup:"
_I18N_zh_PERSIST_LAST_BACKUP_LABEL="最后备份:"

_I18N_en_PERSIST_ENABLED="enabled"
_I18N_zh_PERSIST_ENABLED="已启用"

_I18N_en_PERSIST_DISABLED="disabled"
_I18N_zh_PERSIST_DISABLED="已禁用"

_I18N_en_PERSIST_ENABLE_HINT="Enable: workhive config → Session Persistence"
_I18N_zh_PERSIST_ENABLE_HINT="启用: workhive config → 会话持久化"

_I18N_en_PERSIST_GENERATION_BUMPED_FMT="Persistence generation bumped to %s"
_I18N_zh_PERSIST_GENERATION_BUMPED_FMT="持久化代际已提升到 %s"

# === status snapshot wording ===

_I18N_en_STATUS_SNAPSHOT_TITLE="Session Snapshot Restore"
_I18N_zh_STATUS_SNAPSHOT_TITLE="会话快照恢复"

_I18N_en_STATUS_SNAPSHOT_STATUS_LABEL="Status:"
_I18N_zh_STATUS_SNAPSHOT_STATUS_LABEL="状态:"

_I18N_en_STATUS_SNAPSHOT_AUTOSAVE_LABEL="Auto-snapshot:"
_I18N_zh_STATUS_SNAPSHOT_AUTOSAVE_LABEL="自动快照:"

_I18N_en_STATUS_SNAPSHOT_LAST_BACKUP_LABEL="Latest snapshot:"
_I18N_zh_STATUS_SNAPSHOT_LAST_BACKUP_LABEL="最近快照:"

_I18N_en_STATUS_SNAPSHOT_ENABLE_HINT="Enable: workhive config → Session Persistence (snapshot-based)"
_I18N_zh_STATUS_SNAPSHOT_ENABLE_HINT="启用: workhive config → 会话持久化（基于快照）"

# === pane send subcommand ===

_I18N_en_PANE_SEND_USAGE="Usage: workhive pane send <pane_index|idx1,idx2,...> <text> | workhive pane send --all <text>"
_I18N_zh_PANE_SEND_USAGE="用法: workhive pane send <pane索引|索引1,索引2,...> <文本> | workhive pane send --all <文本>"

_I18N_en_PANE_SEND_INVALID_INDEX="Pane index %s does not exist (max: %s)"
_I18N_zh_PANE_SEND_INVALID_INDEX="Pane 索引 %s 不存在 (最大: %s)"

_I18N_en_PANE_SEND_OK="Text sent to"
_I18N_zh_PANE_SEND_OK="文本已发送至"

_I18N_en_PANE_SEND_BROADCAST_OK="Text broadcast to %s panes"
_I18N_zh_PANE_SEND_BROADCAST_OK="文本已广播到 %s 个 pane"

_I18N_en_PANE_SEND_INVALID_MODE_FMT="Invalid --mode: %s (allowed: auto, plain, paste, bracketed-paste)"
_I18N_zh_PANE_SEND_INVALID_MODE_FMT="无效的 --mode: %s (可选: auto, plain, paste, bracketed-paste)"

_I18N_en_PANE_SEND_INVALID_COMMIT_FMT="Invalid --commit: %s (allowed: auto, enter, c-m, none)"
_I18N_zh_PANE_SEND_INVALID_COMMIT_FMT="无效的 --commit: %s (可选: auto, enter, c-m, none)"

_I18N_en_PANE_SEND_VERBOSE_FMT="pane %s strategy: %s"
_I18N_zh_PANE_SEND_VERBOSE_FMT="pane %s 发送策略: %s"

_I18N_en_PANE_SEND_TARGET_CONFLICT="Cannot combine --all with pane index or --role"
_I18N_zh_PANE_SEND_TARGET_CONFLICT="--all 不能与 pane 索引或 --role 同时使用"

_I18N_en_PANE_SEND_DIAG_NO_BROADCAST="--diagnose does not support --all broadcast mode"
_I18N_zh_PANE_SEND_DIAG_NO_BROADCAST="--diagnose 不支持 --all 广播模式"

_I18N_en_PANE_SEND_DIAG_TITLE="Pane Send Diagnose"
_I18N_zh_PANE_SEND_DIAG_TITLE="Pane 发送诊断"

_I18N_en_PANE_SEND_DIAG_TARGET_FMT="Target pane: %s"
_I18N_zh_PANE_SEND_DIAG_TARGET_FMT="目标窗格: %s"

_I18N_en_PANE_SEND_DIAG_ROLE_FMT="Role: %s"
_I18N_zh_PANE_SEND_DIAG_ROLE_FMT="角色: %s"

_I18N_en_PANE_SEND_DIAG_CMD_LABEL="Command:"
_I18N_zh_PANE_SEND_DIAG_CMD_LABEL="命令:"

_I18N_en_PANE_SEND_DIAG_DIR_LABEL="Directory:"
_I18N_zh_PANE_SEND_DIAG_DIR_LABEL="目录:"

_I18N_en_PANE_SEND_DIAG_STATE_LABEL="Pane state:"
_I18N_zh_PANE_SEND_DIAG_STATE_LABEL="窗格状态:"

_I18N_en_PANE_SEND_DIAG_TEXT_LEN_FMT="Text length: %s"
_I18N_zh_PANE_SEND_DIAG_TEXT_LEN_FMT="文本长度: %s"

_I18N_en_PANE_SEND_DIAG_WITH_ENTER="Send mode: with Enter (default)"
_I18N_zh_PANE_SEND_DIAG_WITH_ENTER="发送模式: 附带回车 (默认)"

_I18N_en_PANE_SEND_DIAG_WARN_EMPTY_TEXT="No text provided for smoke test; diagnose only checks pane state."
_I18N_zh_PANE_SEND_DIAG_WARN_EMPTY_TEXT="未提供文本进行冒烟测试；当前仅检查窗格状态。"

_I18N_en_PANE_SEND_DIAG_WARN_NO_ENTER="Send mode: --commit none (text will not be submitted)"
_I18N_zh_PANE_SEND_DIAG_WARN_NO_ENTER="发送模式: --commit none (文本不会提交执行)"

_I18N_en_PANE_SEND_DIAG_WARN_PANE_DEAD="Target pane is dead; sending text will fail."
_I18N_zh_PANE_SEND_DIAG_WARN_PANE_DEAD="目标窗格已终止；发送文本会失败。"

_I18N_en_PANE_SEND_DIAG_WARN_IN_MODE="Pane is in special mode (e.g., copy-mode); keys may be intercepted."
_I18N_zh_PANE_SEND_DIAG_WARN_IN_MODE="窗格处于特殊模式 (如 copy-mode)；按键可能被拦截。"

_I18N_en_PANE_SEND_DIAG_WARN_INTERACTIVE_FMT="Foreground command '%s' is interactive; Enter may be consumed by that program."
_I18N_zh_PANE_SEND_DIAG_WARN_INTERACTIVE_FMT="前台命令 '%s' 为交互程序；回车可能被该程序消费。"

_I18N_en_PANE_SEND_DIAG_OK="No obvious blockers detected for pane send."
_I18N_zh_PANE_SEND_DIAG_OK="未检测到 pane send 的明显阻塞因素。"

_I18N_en_PANE_SEND_DIAG_CONCLUSION_LABEL="Conclusion:"
_I18N_zh_PANE_SEND_DIAG_CONCLUSION_LABEL="结论:"

_I18N_en_PANE_SEND_DIAG_CONCLUSION_READY="Pane send path looks healthy; default send should execute immediately."
_I18N_zh_PANE_SEND_DIAG_CONCLUSION_READY="pane send 链路看起来正常；默认发送应立即执行。"

_I18N_en_PANE_SEND_DIAG_CONCLUSION_DEAD="Target pane is dead; sending cannot succeed until pane/session is recreated."
_I18N_zh_PANE_SEND_DIAG_CONCLUSION_DEAD="目标窗格已终止；在重建 pane/会话前发送不会成功。"

_I18N_en_PANE_SEND_DIAG_CONCLUSION_IN_MODE="Pane is in special mode (copy-mode etc.); exit mode first, then retry send."
_I18N_zh_PANE_SEND_DIAG_CONCLUSION_IN_MODE="窗格处于特殊模式 (如 copy-mode)；先退出该模式再重试发送。"

_I18N_en_PANE_SEND_DIAG_CONCLUSION_NO_ENTER="Current option is --commit none, so text will not execute automatically."
_I18N_zh_PANE_SEND_DIAG_CONCLUSION_NO_ENTER="当前使用 --commit none，文本不会自动执行。"

_I18N_en_PANE_SEND_DIAG_CONCLUSION_INTERACTIVE_FMT="Foreground command '%s' likely consumes Enter; verify in-shell with 'echo __WH_OK__'."
_I18N_zh_PANE_SEND_DIAG_CONCLUSION_INTERACTIVE_FMT="前台命令 '%s' 可能会消费回车；可先用 'echo __WH_OK__' 验证。"

_I18N_en_PANE_SEND_DIAG_CONCLUSION_EMPTY_TEXT="No text provided; diagnose cannot verify execution behavior."
_I18N_zh_PANE_SEND_DIAG_CONCLUSION_EMPTY_TEXT="未提供文本；当前诊断无法验证执行行为。"

_I18N_en_PANE_SEND_DIAG_HINT_TITLE="Suggested checks:"
_I18N_zh_PANE_SEND_DIAG_HINT_TITLE="建议检查:"

_I18N_en_PANE_SEND_DIAG_HINT_SMOKE_FMT="workhive pane send %s \"echo __WH_OK__\""
_I18N_zh_PANE_SEND_DIAG_HINT_SMOKE_FMT="workhive pane send %s \"echo __WH_OK__\""

_I18N_en_PANE_SEND_DIAG_HINT_SEND_FMT="workhive pane send %s \"your command\""
_I18N_zh_PANE_SEND_DIAG_HINT_SEND_FMT="workhive pane send %s \"你的命令\""

_I18N_en_PANE_SEND_DIAG_HINT_NO_ENTER_FMT="workhive pane send %s \"your text\" --commit none"
_I18N_zh_PANE_SEND_DIAG_HINT_NO_ENTER_FMT="workhive pane send %s \"你的文本\" --commit none"

_I18N_en_PANE_CLOSE_INVALID_INDEX="Pane index %s does not exist (max: %s)"
_I18N_zh_PANE_CLOSE_INVALID_INDEX="Pane 索引 %s 不存在 (最大: %s)"

_I18N_en_PANE_CLOSE_ID_REQUIRES_VALUE="--id requires a value"
_I18N_zh_PANE_CLOSE_ID_REQUIRES_VALUE="--id 需要一个值"

_I18N_en_PANE_CLOSE_TARGET_CONFLICT="pane index and --id cannot be used together"
_I18N_zh_PANE_CLOSE_TARGET_CONFLICT="pane index 与 --id 不能同时使用"

_I18N_en_PANE_CLOSE_INVALID_PANE_ID="Invalid pane id: %s"
_I18N_zh_PANE_CLOSE_INVALID_PANE_ID="无效的 pane id: %s"

_I18N_en_PANE_CLOSE_PANE_ID_NOT_FOUND="Pane id not found in tmux: %s"
_I18N_zh_PANE_CLOSE_PANE_ID_NOT_FOUND="tmux 中未找到 pane id: %s"

_I18N_en_PANE_CLOSE_CROSS_SESSION="Pane id belongs to another session: %s"
_I18N_zh_PANE_CLOSE_CROSS_SESSION="pane id 属于另一个会话: %s"

_I18N_en_PANE_CLOSE_VERIFY_FAILED="Pane close verification failed"
_I18N_zh_PANE_CLOSE_VERIFY_FAILED="pane 关闭校验失败"

_I18N_en_PANE_LIST_INVALID_FORMAT="Invalid pane list format: %s (table, json)"
_I18N_zh_PANE_LIST_INVALID_FORMAT="无效的 pane list 格式: %s (table, json)"

_I18N_en_PANE_CURRENT_INVALID_FORMAT="Invalid pane current format: %s (table, json)"
_I18N_zh_PANE_CURRENT_INVALID_FORMAT="无效的 pane current 格式: %s (table, json)"

# === pane list subcommand ===

_I18N_en_PANE_LIST_TITLE="Panes in current session"
_I18N_zh_PANE_LIST_TITLE="当前会话的 Pane 列表"

_I18N_en_PANE_LIST_EMPTY="No panes found"
_I18N_zh_PANE_LIST_EMPTY="未找到任何 pane"

_I18N_en_PANE_LIST_HDR_INDEX="Index"
_I18N_zh_PANE_LIST_HDR_INDEX="索引"

_I18N_en_PANE_LIST_HDR_CMD="Command"
_I18N_zh_PANE_LIST_HDR_CMD="命令"

_I18N_en_PANE_LIST_HDR_DIR="Directory"
_I18N_zh_PANE_LIST_HDR_DIR="目录"

_I18N_en_PANE_LIST_HDR_SIZE="Size"
_I18N_zh_PANE_LIST_HDR_SIZE="大小"

_I18N_en_PANE_LIST_HDR_STATUS="Status"
_I18N_zh_PANE_LIST_HDR_STATUS="状态"

# === pane current subcommand ===

_I18N_en_PANE_CURRENT_TITLE="Current pane details"
_I18N_zh_PANE_CURRENT_TITLE="当前 Pane 详情"

_I18N_en_PANE_CURRENT_NOT_FOUND="Current pane not found in session (index: %s)"
_I18N_zh_PANE_CURRENT_NOT_FOUND="在会话中找不到当前 pane (索引: %s)"

_I18N_en_PANE_CURRENT_LABEL_INDEX="Index:"
_I18N_zh_PANE_CURRENT_LABEL_INDEX="索引:"

_I18N_en_PANE_CURRENT_LABEL_ID="Pane ID:"
_I18N_zh_PANE_CURRENT_LABEL_ID="Pane ID:"

_I18N_en_PANE_CURRENT_LABEL_CMD="Command:"
_I18N_zh_PANE_CURRENT_LABEL_CMD="命令:"

_I18N_en_PANE_CURRENT_LABEL_DIR="Directory:"
_I18N_zh_PANE_CURRENT_LABEL_DIR="目录:"

_I18N_en_PANE_CURRENT_LABEL_PATH="Path:"
_I18N_zh_PANE_CURRENT_LABEL_PATH="路径:"

_I18N_en_PANE_CURRENT_LABEL_SIZE="Size:"
_I18N_zh_PANE_CURRENT_LABEL_SIZE="尺寸:"

_I18N_en_PANE_CURRENT_LABEL_STATUS="Status:"
_I18N_zh_PANE_CURRENT_LABEL_STATUS="状态:"

_I18N_en_PANE_CURRENT_LABEL_AGENT="Agent:"
_I18N_zh_PANE_CURRENT_LABEL_AGENT="Agent:"

_I18N_en_PANE_CURRENT_LABEL_AGENT_STATUS="Agent Status:"
_I18N_zh_PANE_CURRENT_LABEL_AGENT_STATUS="Agent 状态:"

_I18N_en_PANE_CURRENT_LABEL_STATE_SOURCE="State Source:"
_I18N_zh_PANE_CURRENT_LABEL_STATE_SOURCE="状态来源:"

_I18N_en_PANE_CURRENT_LABEL_STATE_CONFIDENCE="State Confidence:"
_I18N_zh_PANE_CURRENT_LABEL_STATE_CONFIDENCE="状态置信度:"

_I18N_en_PANE_CURRENT_LABEL_STATE_REASON="State Reason:"
_I18N_zh_PANE_CURRENT_LABEL_STATE_REASON="状态原因:"

# === columns layout mode ===

_I18N_en_COLUMNS_PANE_LABEL="Col %s Pane %s"
_I18N_zh_COLUMNS_PANE_LABEL="列 %s 窗格 %s"

_I18N_en_MSG_LAYOUT_MODE_COLUMNS="columns (flexible column-based layout)"
_I18N_zh_MSG_LAYOUT_MODE_COLUMNS="columns (灵活的列式布局)"

_I18N_en_LAYOUT_COLUMNS="Columns"
_I18N_zh_LAYOUT_COLUMNS="列式布局"

_I18N_en_LAYOUT_COLUMNS_DESC="Flexible column-based layout — each column has independent pane count"
_I18N_zh_LAYOUT_COLUMNS_DESC="灵活的列式布局 — 每列可有不同数量的窗格"

_I18N_en_CUSTOM_EACH_COL_INDEPENDENT="(each col has independent pane count)"
_I18N_zh_CUSTOM_EACH_COL_INDEPENDENT="(每列可有不同数量的窗格)"

_I18N_en_CUSTOM_TPL_DESC="Custom columns — define columns and panes freely"
_I18N_zh_CUSTOM_TPL_DESC="自定义列数 — 自由定义列和窗格"

_I18N_en_CUSTOM_NUM_COLUMNS_PROMPT="Number of columns (1-5):"
_I18N_zh_CUSTOM_NUM_COLUMNS_PROMPT="列数 (1-5):"

_I18N_en_CUSTOM_INVALID_COLUMNS="Invalid: enter 1-5"
_I18N_zh_CUSTOM_INVALID_COLUMNS="无效输入: 请输入 1-5"

_I18N_en_CUSTOM_COL_PANES_PROMPT="Column %s — number of panes (1-%s):"
_I18N_zh_CUSTOM_COL_PANES_PROMPT="第 %s 列 — 窗格数量 (1-%s):"

_I18N_en_CUSTOM_INVALID_PANES="Invalid: enter 1-%s"
_I18N_zh_CUSTOM_INVALID_PANES="无效输入: 请输入 1-%s"

_I18N_en_INIT_COLUMNS_LAYOUT_SUMMARY_FMT="columns (%s columns)"
_I18N_zh_INIT_COLUMNS_LAYOUT_SUMMARY_FMT="columns (%s 列)"

_I18N_en_CUSTOM_CURRENT_COLUMNS_FMT="%s columns"
_I18N_zh_CUSTOM_CURRENT_COLUMNS_FMT="%s 列"

# === pane role system ===

_I18N_en_PANE_ROLE_NOT_FOUND="No pane with role: %s"
_I18N_zh_PANE_ROLE_NOT_FOUND="未找到角色为 %s 的窗格"

_I18N_en_PANE_ROLE_LABEL="Role"
_I18N_zh_PANE_ROLE_LABEL="角色"

# === agent status detection ===

_I18N_en_PANE_LIST_HDR_AGENT="Agent"
_I18N_zh_PANE_LIST_HDR_AGENT="AI Agent"

_I18N_en_PANE_LIST_HDR_AGENT_STATUS="Agent Status"
_I18N_zh_PANE_LIST_HDR_AGENT_STATUS="Agent 状态"

_I18N_en_AGENT_STATUS_RUNNING="running"
_I18N_zh_AGENT_STATUS_RUNNING="运行中"

_I18N_en_AGENT_STATUS_WAITING="waiting"
_I18N_zh_AGENT_STATUS_WAITING="等待输入"

_I18N_en_AGENT_STATUS_STOPPED="stopped"
_I18N_zh_AGENT_STATUS_STOPPED="已停止"

_I18N_en_AGENT_STATUS_IDLE="idle"
_I18N_zh_AGENT_STATUS_IDLE="空闲"

_I18N_en_AGENT_STATUS_UNKNOWN="unknown"
_I18N_zh_AGENT_STATUS_UNKNOWN="未知"

_I18N_en_AGENT_STATUS_NONE="—"
_I18N_zh_AGENT_STATUS_NONE="—"

_I18N_en_AGENT_NOTIFY_FINISHED="Agent %s (pane %s) finished"
_I18N_zh_AGENT_NOTIFY_FINISHED="Agent %s (窗格 %s) 已完成"

_I18N_en_AGENT_NOTIFY_WAITING="Agent %s (pane %s) needs input"
_I18N_zh_AGENT_NOTIFY_WAITING="Agent %s (窗格 %s) 需要输入"

# === shell integration command ===

_I18N_en_HELP_CMD_SHELL="manage unified shell setup"
_I18N_zh_HELP_CMD_SHELL="统一管理 shell 配置"

_I18N_en_SHELL_UNSUPPORTED="Unsupported shell:"
_I18N_zh_SHELL_UNSUPPORTED="不支持的 shell:"

_I18N_en_SHELL_SCRIPT_NOT_FOUND="Shell integration script not found:"
_I18N_zh_SHELL_SCRIPT_NOT_FOUND="Shell 集成脚本未找到:"

_I18N_en_SHELL_DRY_RUN_HEADER="Dry run — no changes will be made:"
_I18N_zh_SHELL_DRY_RUN_HEADER="预览模式 — 不会进行任何更改:"

_I18N_en_SHELL_TARGET_FILE="Target file"
_I18N_zh_SHELL_TARGET_FILE="目标文件"

_I18N_en_SHELL_LINE_TO_ADD="Line to add"
_I18N_zh_SHELL_LINE_TO_ADD="将要添加的行"

_I18N_en_SHELL_ALREADY_INSTALLED="workhive shell integration is already installed"
_I18N_zh_SHELL_ALREADY_INSTALLED="workhive shell 集成已安装"

_I18N_en_SHELL_INIT_SUCCESS="Shell integration installed successfully"
_I18N_zh_SHELL_INIT_SUCCESS="Shell 集成安装成功"

_I18N_en_SHELL_INSTALL_TO="Installed to"
_I18N_zh_SHELL_INSTALL_TO="安装到"

_I18N_en_SHELL_SCRIPT_UPDATED="Script updated"
_I18N_zh_SHELL_SCRIPT_UPDATED="脚本已更新"

_I18N_en_SHELL_RESTART_HINT="Restart your shell to apply changes:"
_I18N_zh_SHELL_RESTART_HINT="重启 shell 以应用更改:"

_I18N_en_SHELL_NOT_INSTALLED="workhive shell integration is not installed"
_I18N_zh_SHELL_NOT_INSTALLED="workhive shell 集成未安装"

_I18N_en_SHELL_REMOVE_FROM="Will remove from"
_I18N_zh_SHELL_REMOVE_FROM="将从以下文件移除"

_I18N_en_SHELL_REMOVE_SUCCESS="Shell integration removed successfully"
_I18N_zh_SHELL_REMOVE_SUCCESS="Shell 集成已成功移除"

_I18N_en_ERR_MISSING_VALUE_FOR_OPT_FMT="Missing value for %s"
_I18N_zh_ERR_MISSING_VALUE_FOR_OPT_FMT="缺少参数值: %s"

_I18N_en_SHELL_PROFILE_MINIMAL_DESC="Minimal profile: only essential shell integration with completion support."
_I18N_zh_SHELL_PROFILE_MINIMAL_DESC="Minimal 配置: 仅包含必要的 shell 集成与补全支持。"

_I18N_en_SHELL_PROFILE_STANDARD_DESC="Standard profile: minimal + common aliases and history settings."
_I18N_zh_SHELL_PROFILE_STANDARD_DESC="Standard 配置: 在 minimal 基础上增加常用别名和历史设置。"

_I18N_en_SHELL_PROFILE_ADVANCED_DESC="Advanced profile: standard + optional productivity integrations."
_I18N_zh_SHELL_PROFILE_ADVANCED_DESC="Advanced 配置: 在 standard 基础上增加可选效率集成。"

_I18N_en_SHELL_PROFILE_UNKNOWN="Unknown profile."
_I18N_zh_SHELL_PROFILE_UNKNOWN="未知配置档位。"

_I18N_en_SHELL_MODULE_COMPLETIONS_DESC="Enable command completion for workhive in the target shell."
_I18N_zh_SHELL_MODULE_COMPLETIONS_DESC="在目标 shell 中启用 workhive 命令补全。"

_I18N_en_SHELL_MODULE_ALIASES_DESC="Add lightweight convenience aliases (ll/la/g/gst)."
_I18N_zh_SHELL_MODULE_ALIASES_DESC="添加轻量便捷别名 (ll/la/g/gst)。"

_I18N_en_SHELL_MODULE_HISTORY_DESC="Tune shell history behavior (size, append/share behavior)."
_I18N_zh_SHELL_MODULE_HISTORY_DESC="优化 shell 历史行为 (大小、追加/共享策略)。"

_I18N_en_SHELL_MODULE_ZOXIDE_DESC="Enable zoxide integration if installed."
_I18N_zh_SHELL_MODULE_ZOXIDE_DESC="若已安装 zoxide，则启用其集成。"

_I18N_en_SHELL_MODULE_STARSHIP_DESC="Enable starship prompt integration if installed."
_I18N_zh_SHELL_MODULE_STARSHIP_DESC="若已安装 starship，则启用其提示符集成。"

_I18N_en_SHELL_MODULE_SSH_TERM_FIX_DESC="Force TERM=xterm-256color for ssh when inside tmux-256color."
_I18N_zh_SHELL_MODULE_SSH_TERM_FIX_DESC="在 tmux-256color 下执行 ssh 时强制 TERM=xterm-256color。"

_I18N_en_SHELL_MODULE_UNKNOWN_DESC="Unknown module."
_I18N_zh_SHELL_MODULE_UNKNOWN_DESC="未知模块。"

_I18N_en_SHELL_ERR_UNKNOWN_MODULE_FMT="Unknown shell module: %s"
_I18N_zh_SHELL_ERR_UNKNOWN_MODULE_FMT="未知 shell 模块: %s"

_I18N_en_SHELL_SELECT_PROFILE="Select shell profile:"
_I18N_zh_SHELL_SELECT_PROFILE="选择 shell 配置档位:"

_I18N_en_SHELL_RECOMMENDED_DEFAULT="recommended default"
_I18N_zh_SHELL_RECOMMENDED_DEFAULT="推荐默认值"

_I18N_en_SHELL_SELECT_PROFILE_CHOOSE="Choose [1/2/3] (default 1):"
_I18N_zh_SHELL_SELECT_PROFILE_CHOOSE="请选择 [1/2/3] (默认 1):"

_I18N_en_SHELL_INVALID_CHOICE="Invalid choice."
_I18N_zh_SHELL_INVALID_CHOICE="无效选择。"

_I18N_en_SHELL_ASK_USE_PROFILE_MINIMAL="Use profile 'minimal'?"
_I18N_zh_SHELL_ASK_USE_PROFILE_MINIMAL="使用 'minimal' 配置吗?"

_I18N_en_SHELL_ASK_USE_PROFILE_STANDARD="Use profile 'standard'?"
_I18N_zh_SHELL_ASK_USE_PROFILE_STANDARD="使用 'standard' 配置吗?"

_I18N_en_SHELL_ASK_USE_PROFILE_ADVANCED="Use profile 'advanced'?"
_I18N_zh_SHELL_ASK_USE_PROFILE_ADVANCED="使用 'advanced' 配置吗?"

_I18N_en_SHELL_MODULE_DESC_FMT="Module '%s': %s"
_I18N_zh_SHELL_MODULE_DESC_FMT="模块 '%s': %s"

_I18N_en_SHELL_ASK_ENABLE_MODULE_FMT="Enable module '%s'?"
_I18N_zh_SHELL_ASK_ENABLE_MODULE_FMT="启用模块 '%s' 吗?"

_I18N_en_SHELL_ASK_CUSTOMIZE_MODULES="Customize modules one by one?"
_I18N_zh_SHELL_ASK_CUSTOMIZE_MODULES="是否逐项自定义模块?"

_I18N_en_SHELL_MODULE_DEP_INSTALLED_FMT="Dependency '%s': installed"
_I18N_zh_SHELL_MODULE_DEP_INSTALLED_FMT="依赖 '%s': 已安装"

_I18N_en_SHELL_MODULE_DEP_MISSING_FMT="Dependency '%s': not installed"
_I18N_zh_SHELL_MODULE_DEP_MISSING_FMT="依赖 '%s': 未安装"

_I18N_en_SHELL_MODULE_DEP_INSTALL_HINT_FMT="Install hint: %s"
_I18N_zh_SHELL_MODULE_DEP_INSTALL_HINT_FMT="安装建议: %s"

_I18N_en_SHELL_MODULE_MISSING_DEPS_HEADER="Some enabled modules require tools that are not installed yet:"
_I18N_zh_SHELL_MODULE_MISSING_DEPS_HEADER="以下已启用模块依赖的工具尚未安装:"

_I18N_en_SHELL_MODULE_MISSING_DEPS_ITEM_FMT="module '%s' needs '%s' (hint: %s)"
_I18N_zh_SHELL_MODULE_MISSING_DEPS_ITEM_FMT="模块 '%s' 需要 '%s'（建议: %s）"

_I18N_en_SHELL_ERR_INVALID_PROFILE_FMT="Invalid --profile: %s (allowed: minimal, standard, advanced)"
_I18N_zh_SHELL_ERR_INVALID_PROFILE_FMT="无效的 --profile: %s (可选: minimal, standard, advanced)"

_I18N_en_SHELL_LABEL_PROFILE="Profile"
_I18N_zh_SHELL_LABEL_PROFILE="配置档位"

_I18N_en_SHELL_LABEL_MODULES="Modules"
_I18N_zh_SHELL_LABEL_MODULES="模块"

_I18N_en_SHELL_LABEL_NONE="<none>"
_I18N_zh_SHELL_LABEL_NONE="<无>"

# === command runtime i18n hardening ===

_I18N_en_ERR_CMD_NOT_FOUND="Command not found:"
_I18N_zh_ERR_CMD_NOT_FOUND="命令不存在:"

_I18N_en_ERR_DIR_NOT_EXIST="Directory does not exist:"
_I18N_zh_ERR_DIR_NOT_EXIST="目录不存在:"

_I18N_en_ERR_CD_FAILED_TO="Failed to cd to"
_I18N_zh_ERR_CD_FAILED_TO="切换目录失败:"

_I18N_en_PANE_ERR_REJECTED_UNSAFE_CMD="Rejected unsafe command: shell metacharacters are not allowed"
_I18N_zh_PANE_ERR_REJECTED_UNSAFE_CMD="已拒绝不安全命令: 不允许使用 shell 元字符"

_I18N_en_PANE_ERR_PRESET_NOT_ALLOWLIST_FMT="Rejected preset command: '%s' is not in the enterprise allowlist"
_I18N_zh_PANE_ERR_PRESET_NOT_ALLOWLIST_FMT="已拒绝预设命令: '%s' 不在企业白名单中"

_I18N_en_PANE_ERR_PRESET_UNTRUSTED="Rejected untrusted preset command: not from tools registry"
_I18N_zh_PANE_ERR_PRESET_UNTRUSTED="已拒绝不受信任的预设命令: 非 tools 注册表来源"

_I18N_en_CONFIG_GIT_WHITELIST_USAGE_ADD="Usage: workhive config git-whitelist add <path>"
_I18N_zh_CONFIG_GIT_WHITELIST_USAGE_ADD="用法: workhive config git-whitelist add <path>"

_I18N_en_CONFIG_GIT_WHITELIST_USAGE_EDIT="Usage: workhive config git-whitelist edit <old-path> <new-path>"
_I18N_zh_CONFIG_GIT_WHITELIST_USAGE_EDIT="用法: workhive config git-whitelist edit <旧路径> <新路径>"

_I18N_en_CONFIG_GIT_WHITELIST_USAGE_MANAGE="Usage: workhive config git-whitelist manage"
_I18N_zh_CONFIG_GIT_WHITELIST_USAGE_MANAGE="用法: workhive config git-whitelist manage"

_I18N_en_CONFIG_GIT_WHITELIST_MANAGE_HELP_PURPOSE="Purpose: Open an interactive manager for git init whitelist paths."
_I18N_zh_CONFIG_GIT_WHITELIST_MANAGE_HELP_PURPOSE="用途: 打开 git init 白名单路径的交互式管理器。"

_I18N_en_CONFIG_GIT_WHITELIST_MANAGE_HELP_ACTIONS="Available actions:"
_I18N_zh_CONFIG_GIT_WHITELIST_MANAGE_HELP_ACTIONS="可执行操作:"

_I18N_en_CONFIG_GIT_WHITELIST_MANAGE_HELP_NOTE="Use Enter to select an action; only choosing 'Done' exits manager."
_I18N_zh_CONFIG_GIT_WHITELIST_MANAGE_HELP_NOTE="按 Enter 选择操作；只有选择“完成”才会退出管理器。"

_I18N_en_CONFIG_GIT_WHITELIST_USAGE_REMOVE="Usage: workhive config git-whitelist remove <path>"
_I18N_zh_CONFIG_GIT_WHITELIST_USAGE_REMOVE="用法: workhive config git-whitelist remove <path>"

_I18N_en_CONFIG_GIT_WHITELIST_UNKNOWN_ACTION="Unknown git-whitelist action:"
_I18N_zh_CONFIG_GIT_WHITELIST_UNKNOWN_ACTION="未知 git-whitelist 操作:"

_I18N_en_CONFIG_GIT_WHITELIST_USAGE_ALL="Usage: workhive config git-whitelist [add <path>|edit <old-path> <new-path>|remove <path>|list|manage]"
_I18N_zh_CONFIG_GIT_WHITELIST_USAGE_ALL="用法: workhive config git-whitelist [add <path>|edit <旧路径> <新路径>|remove <path>|list|manage]"

_I18N_en_CONFIG_GIT_WHITELIST_HELP_ADD="Add a path to git init whitelist"
_I18N_zh_CONFIG_GIT_WHITELIST_HELP_ADD="将路径加入 git init 白名单"

_I18N_en_CONFIG_GIT_WHITELIST_HELP_EDIT="Modify an existing whitelist path"
_I18N_zh_CONFIG_GIT_WHITELIST_HELP_EDIT="修改已有白名单路径"

_I18N_en_CONFIG_GIT_WHITELIST_HELP_REMOVE="Remove a path from git init whitelist"
_I18N_zh_CONFIG_GIT_WHITELIST_HELP_REMOVE="从 git init 白名单删除路径"

_I18N_en_CONFIG_GIT_WHITELIST_HELP_LIST="List effective whitelist paths"
_I18N_zh_CONFIG_GIT_WHITELIST_HELP_LIST="查看当前生效的白名单路径"

_I18N_en_CONFIG_GIT_WHITELIST_HELP_MANAGE="Open interactive whitelist manager"
_I18N_zh_CONFIG_GIT_WHITELIST_HELP_MANAGE="打开交互式白名单管理器"

_I18N_en_CONFIG_GIT_WHITELIST_HELP_HELP="Show this help"
_I18N_zh_CONFIG_GIT_WHITELIST_HELP_HELP="显示此帮助"

_I18N_en_UPGRADE_ERR_INVALID_VERSION_FMT="Invalid version format: '%s' (expected: X.Y.Z)"
_I18N_zh_UPGRADE_ERR_INVALID_VERSION_FMT="版本格式无效: '%s' (期望: X.Y.Z)"

_I18N_en_UPGRADE_ERR_INVALID_VERSION_EXAMPLE_FMT="Invalid version format: '%s' (expected: X.Y.Z, e.g. 0.7.0)"
_I18N_zh_UPGRADE_ERR_INVALID_VERSION_EXAMPLE_FMT="版本格式无效: '%s' (期望: X.Y.Z，例如 0.7.0)"

_I18N_en_LOG_ERR_INVALID_LINES_FMT="Invalid --lines value: %s (must be positive integer)"
_I18N_zh_LOG_ERR_INVALID_LINES_FMT="无效的 --lines 值: %s (必须为正整数)"

_I18N_en_LOG_ERR_INVALID_LEVEL_FMT="Invalid --level: %s (use: debug, info, warn, error)"
_I18N_zh_LOG_ERR_INVALID_LEVEL_FMT="无效的 --level: %s (可选: debug, info, warn, error)"

_I18N_en_LOG_FILE_CLEARED="Log file cleared."
_I18N_zh_LOG_FILE_CLEARED="日志文件已清空。"

_I18N_en_GC_WARN_KILL_SESSION_FAILED="Failed to kill session for:"
_I18N_zh_GC_WARN_KILL_SESSION_FAILED="结束会话失败:"

_I18N_en_GC_WARN_REMOVE_WORKTREE_FAILED="Failed to remove worktree:"
_I18N_zh_GC_WARN_REMOVE_WORKTREE_FAILED="删除 worktree 失败:"

_I18N_en_GC_WARN_BRANCH_DELETE_SKIPPED_FMT="Branch '%s' could not be deleted. Skipping."
_I18N_zh_GC_WARN_BRANCH_DELETE_SKIPPED_FMT="分支 '%s' 无法删除，已跳过。"

_I18N_en_GC_WARN_BRANCH_NOT_MERGED_FMT="Branch '%s' not fully merged into %s. Skipping."
_I18N_zh_GC_WARN_BRANCH_NOT_MERGED_FMT="分支 '%s' 未完全合并到 %s，已跳过。"

_I18N_en_GC_WARN_DELETE_BRANCH_FAILED="Failed to delete branch:"
_I18N_zh_GC_WARN_DELETE_BRANCH_FAILED="删除分支失败:"

_I18N_en_GC_WARN_SKIP_DIRTY_AUTO="Skipping branch with uncommitted changes (--auto safety):"
_I18N_zh_GC_WARN_SKIP_DIRTY_AUTO="跳过含未提交更改的分支 (--auto 安全保护):"

_I18N_en_GC_WARN_KILL_ORPHAN_FAILED="Failed to kill orphan session:"
_I18N_zh_GC_WARN_KILL_ORPHAN_FAILED="结束孤立会话失败:"

_I18N_en_GC_INFO_SESSION_NOT_FOUND="Session not found (already cleaned):"
_I18N_zh_GC_INFO_SESSION_NOT_FOUND="会话不存在 (可能已清理):"

_I18N_en_GC_WARN_REMOVE_STATE_FAILED="Failed to remove stale state file:"
_I18N_zh_GC_WARN_REMOVE_STATE_FAILED="删除过期状态文件失败:"

_I18N_en_GC_WARN_REMOVE_SNAPSHOT_FAILED="Failed to remove snapshot file:"
_I18N_zh_GC_WARN_REMOVE_SNAPSHOT_FAILED="删除快照文件失败:"

_I18N_en_MSG_NO_ACTIVE_WORKSPACES="No workspaces found"
_I18N_zh_MSG_NO_ACTIVE_WORKSPACES="当前没有可显示的工作空间"

_I18N_en_STATUS_REFRESHING_EVERY="Refreshing every "
_I18N_zh_STATUS_REFRESHING_EVERY="每隔 "

_I18N_en_STATUS_SECONDS="s..."
_I18N_zh_STATUS_SECONDS=" 秒刷新..."

_I18N_en_STATUS_CTRL_C_STOP="Ctrl+C to stop"
_I18N_zh_STATUS_CTRL_C_STOP="按 Ctrl+C 停止"

_I18N_en_DESTROY_WARN_FORCE_DELETE_WT_FAILED="Failed to force-delete worktree:"
_I18N_zh_DESTROY_WARN_FORCE_DELETE_WT_FAILED="强制删除 worktree 失败:"

_I18N_en_COMPLETIONS_ERR_UNSUPPORTED_SHELL="Unsupported shell (supported: zsh, bash, fish):"
_I18N_zh_COMPLETIONS_ERR_UNSUPPORTED_SHELL="不支持的 shell (支持: zsh, bash, fish):"

_I18N_en_BIN_ERR_UNEXPECTED_FMT="Unexpected error at %s:%s"
_I18N_zh_BIN_ERR_UNEXPECTED_FMT="发生未预期错误: %s:%s"

_I18N_en_BIN_ERR_TMUX_REQUIRED_FMT="%s requires tmux, but tmux is not installed."
_I18N_zh_BIN_ERR_TMUX_REQUIRED_FMT="%s 依赖 tmux，但当前未安装 tmux。"

_I18N_en_BIN_HINT_INSTALL_TMUX="Install: brew install tmux (macOS) or apt install tmux (Linux)"
_I18N_zh_BIN_HINT_INSTALL_TMUX="安装方式: brew install tmux (macOS) 或 apt install tmux (Linux)"
_I18N_en_BIN_HINT_USE_PANE_SPLIT="Hint: use 'workhive pane split'"
_I18N_zh_BIN_HINT_USE_PANE_SPLIT="提示: 请使用 'workhive pane split'"
_I18N_en_BIN_HINT_USE_CONFIG_LAYOUT="Hint: use 'workhive config --layout'"
_I18N_zh_BIN_HINT_USE_CONFIG_LAYOUT="提示: 请使用 'workhive config --layout'"

_I18N_en_ERR_WT_BASE_PATH_OUTSIDE_FMT="worktree.base_path must be within project root: %s"
_I18N_zh_ERR_WT_BASE_PATH_OUTSIDE_FMT="worktree.base_path 必须位于项目根目录内: %s"

_I18N_en_ERR_WT_PROJECT_ROOT_FMT="Project root: %s"
_I18N_zh_ERR_WT_PROJECT_ROOT_FMT="项目根目录: %s"

_I18N_en_DEPS_ERR_UNKNOWN_DEP_FMT="Unknown dependency: %s"
_I18N_zh_DEPS_ERR_UNKNOWN_DEP_FMT="未知依赖: %s"

_I18N_en_DEPS_WARN_UPGRADE_UNSUPPORTED_FMT="Dependency '%s' does not support upgrade"
_I18N_zh_DEPS_WARN_UPGRADE_UNSUPPORTED_FMT="依赖 '%s' 不支持升级"

_I18N_en_ERR_WORKSPACE_PREP_EMPTY_PATH="workspace_prepare_directory returned empty path"
_I18N_zh_ERR_WORKSPACE_PREP_EMPTY_PATH="workspace_prepare_directory 返回了空路径"

_I18N_en_ERR_SESSION_SENTINEL_LEAK_FMT="Internal sentinel value leaked as branch name: %s"
_I18N_zh_ERR_SESSION_SENTINEL_LEAK_FMT="内部哨兵值泄漏为分支名: %s"

_I18N_en_ERR_SESSION_CREATE_WINDOW_FAILED_FMT="Failed to create window '%s' in session '%s'"
_I18N_zh_ERR_SESSION_CREATE_WINDOW_FAILED_FMT="创建窗口 '%s' 失败，会话: '%s'"

_I18N_en_ERR_CANNOT_CREATE_DIR_FMT="Cannot create directory: %s (permission denied?)"
_I18N_zh_ERR_CANNOT_CREATE_DIR_FMT="无法创建目录: %s (可能权限不足)"

_I18N_en_ERR_SAFE_RM_EMPTY="Refusing to delete dangerous path: '<empty>'"
_I18N_zh_ERR_SAFE_RM_EMPTY="拒绝删除危险路径: '<empty>'"

_I18N_en_ERR_SAFE_RM_DANGEROUS_FMT="Refusing to delete dangerous path: '%s' (resolved: %s)"
_I18N_zh_ERR_SAFE_RM_DANGEROUS_FMT="拒绝删除危险路径: '%s' (解析后: %s)"

_I18N_en_HINT_INSTALL_YQ="  Install: brew install yq  (or: go install github.com/mikefarah/yq/v4@latest)"
_I18N_zh_HINT_INSTALL_YQ="  安装方式: brew install yq  (或: go install github.com/mikefarah/yq/v4@latest)"
_I18N_en_HINT_YQ_PIN_ENV="  Pin exact version: export WORKHIVE_YQ_REQUIRED_VERSION=4.52.5"
_I18N_zh_HINT_YQ_PIN_ENV="  指定精确版本: export WORKHIVE_YQ_REQUIRED_VERSION=4.52.5"

_I18N_en_WARN_CONFIG_READ_FAILED_FMT="Cannot read config file: %s (permission denied or I/O error)"
_I18N_zh_WARN_CONFIG_READ_FAILED_FMT="无法读取配置文件: %s (权限不足或 I/O 错误)"

_I18N_en_ERR_CONFIG_SET_UNSAFE_KEY_FMT="config_set: rejected unsafe key: %s"
_I18N_zh_ERR_CONFIG_SET_UNSAFE_KEY_FMT="config_set: 拒绝不安全 key: %s"

_I18N_en_PLUGIN_WARN_SYMLINK_OUTSIDE_FMT="Plugin symlink points outside plugins dir: %s -> %s"
_I18N_zh_PLUGIN_WARN_SYMLINK_OUTSIDE_FMT="插件软链接指向 plugins 目录外: %s -> %s"

_I18N_en_PLUGIN_WARN_WORLD_WRITABLE_FMT="Plugin file is world-writable (unsafe permissions): %s"
_I18N_zh_PLUGIN_WARN_WORLD_WRITABLE_FMT="插件文件对所有用户可写 (不安全权限): %s"

_I18N_en_PERSIST_WARN_STALE_LOCK_REMOVING_FMT="Removing stale snapshot save lock: %s"
_I18N_zh_PERSIST_WARN_STALE_LOCK_REMOVING_FMT="移除过期的快照保存锁: %s"

_I18N_en_PERSIST_WARN_SAVE_SKIPPED_LOCKED="Snapshot save skipped: another save is still in progress"
_I18N_zh_PERSIST_WARN_SAVE_SKIPPED_LOCKED="已跳过快照保存: 仍有其他保存任务在进行"

_I18N_en_PERSIST_ERR_CREATE_TEMP_SNAPSHOT="Failed to create temp snapshot file"
_I18N_zh_PERSIST_ERR_CREATE_TEMP_SNAPSHOT="创建临时快照文件失败"

_I18N_en_PERSIST_ERR_FINALIZE_SNAPSHOT="Snapshot finalize failed (disk full / permissions?)"
_I18N_zh_PERSIST_ERR_FINALIZE_SNAPSHOT="快照落盘失败 (可能磁盘已满或权限不足)"

_I18N_en_SNAPSHOT_ERR_MKTEMP_FAILED_FMT="mktemp failed in %s"
_I18N_zh_SNAPSHOT_ERR_MKTEMP_FAILED_FMT="在 %s 执行 mktemp 失败"

_I18N_en_SNAPSHOT_ERR_RENAME_TEMP_FAILED_FMT="Failed to rename temp file to %s"
_I18N_zh_SNAPSHOT_ERR_RENAME_TEMP_FAILED_FMT="重命名临时文件失败: %s"

_I18N_en_SNAPSHOT_WARN_FINALIZE_MOVE_FAILED_FMT="Failed to finalize snapshot: mv %s -> %s"
_I18N_zh_SNAPSHOT_WARN_FINALIZE_MOVE_FAILED_FMT="快照最终落盘失败: mv %s -> %s"
