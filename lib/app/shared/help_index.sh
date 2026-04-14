#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Auto-generated help index for fast --help rendering
# Contains all command help functions (no heavy dependencies)
# Regenerate: see bin/workhive performance notes
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[[ -n "${_HELP_INDEX_LOADED:-}" ]] && return 0
readonly _HELP_INDEX_LOADED=1

init_command_help() {
    _help_dispatch "_init_command_help"
}

_init_command_help_en() {
    cat <<EOF
Usage: workhive init [options]

Interactive project configuration, creates .workhive/config.yaml

Options:
  --non-interactive     Use default config, skip interaction
  --force, -f           Force re-init (overwrite existing config)
  --global              Initialize global configuration
  --debug               Enable debug mode
  -h, --help            Show help

Examples:
  workhive init                    # Interactive config
  workhive init --non-interactive  # Quick init (default config)
  workhive init --force            # Reconfigure (overwrite)
  workhive init --global           # Initialize global config

Config contents:
  • Workflow template (AI Coding Left / AI Coding Right / Quad)
  • Layout and Pane command config
  • tmux theme and visual settings

Generated files:
  .workhive/config.yaml              Project config file
  ~/.config/workhive/config.yaml     Global config file (--global)
EOF
}

_init_command_help_zh() {
    cat <<EOF
用法: workhive init [options]

交互式初始化项目配置,创建 .workhive/config.yaml

选项:
  --non-interactive     使用默认配置,跳过交互
  --force, -f           强制重新初始化 (覆盖现有配置)
  --global              初始化全局配置
  --debug               开启调试模式
  -h, --help            显示帮助信息

示例:
  workhive init                    # 交互式配置
  workhive init --non-interactive  # 快速初始化 (默认配置)
  workhive init --force            # 重新配置 (覆盖)
  workhive init --global           # 初始化全局配置

配置内容:
  • 工作流模板选择 (AI Coding Left / AI Coding Right / Quad)
  • 布局和 Pane 命令配置
  • tmux 主题和视觉设置

生成文件:
  .workhive/config.yaml              项目配置文件
  ~/.config/workhive/config.yaml     全局配置文件 (--global)
EOF
}

open_command_help() {
    _help_dispatch "_open_command_help"
}

_open_command_help_en() {
    cat <<EOF
Usage: workhive open [worktree] [options]

Create or enter a Worktree. If the Worktree does not exist, it will be created automatically.
Supports three modes:

1. Worktree mode (recommended):
   workhive open <worktree>    # Create isolated environment for specified branch

2. Interactive selection (recommended):
   workhive open               # Pop selection menu
                               # Choose existing space, create new Worktree, or open main directory

3. Main directory mode:
   Select "Main directory" in menu to start in project root
   (Cancelling also falls back to main directory)

Arguments:
  <worktree>            Worktree name (Git branch name)
                        If omitted, interactive selection menu appears

Options:
  --force, -f           Force recreate session and layout (destroy old session)
  --force-layout        Force rebuild layout from config (apply config changes)
  --no-attach           Do not auto-attach after creation
  -h, --help            Show help

Examples:
  workhive open feature-x              # Create or enter feature-x Worktree
  workhive open                        # Interactive: select/create/search Worktree
  workhive open dev01 --force-layout   # Rebuild layout with latest config
  workhive open dev01 --no-attach      # Create but don't attach

Difference from attach:
  open   → Auto-creates Worktree + session + layout if not exists
  attach → Only re-enters existing session (creates nothing)
EOF
}

_open_command_help_zh() {
    cat <<EOF
用法: workhive open [worktree] [options]

创建或进入Worktree。如果 Worktree 不存在会自动创建。
支持三种模式:

1. Worktree 模式 (推荐):
   workhive open <worktree>    # 为指定分支创建隔离环境

2. 交互式选择 (推荐):
   workhive open               # 弹出选择菜单
                               # 可选择已有空间、创建新 Worktree、或打开主目录

3. 主目录模式:
   在选择菜单中选择"主目录"，在项目根目录启动环境
   (取消选择时将直接退出)

参数:
  <worktree>            Worktree 名称 (即 Git 分支名)
                        如果省略,弹出交互式选择菜单

选项:
  --force, -f           强制重建会话和布局 (销毁旧会话)
  --force-layout        强制按配置文件重建布局 (修改 config.yaml 后使用)
  --no-attach           创建后不自动进入
  -h, --help            显示帮助信息

示例:
  workhive open feature-x              # 创建或进入 feature-x Worktree
  workhive open                        # 交互式: 选择/创建/搜索Worktree
  workhive open dev01 --force-layout   # 按最新配置重建布局
  workhive open dev01 --no-attach      # 创建但不自动进入

与 attach 的区别:
  open   → 不存在时自动创建 Worktree + 会话 + 布局
  attach → 仅重新进入已有的会话 (不创建任何资源)
EOF
}

attach_command_help() {
    _help_dispatch "_attach_command_help"
}

_attach_command_help_en() {
    cat <<EOF
Usage: workhive attach [session|branch] [options]

Attach to an existing tmux session. This command does not create worktrees
or sessions.

Arguments:
  <session|branch>      Existing tmux session name, or its branch shortcut
                        If omitted, interactive selection menu appears

Options:
  -r, --read-only       Enter in read-only mode (view only, no edit)
  --force-layout        Force rebuild layout from config (apply config changes)
  -h, --help            Show help

Examples:
  workhive attach feature-x              # Attach via branch shortcut
  workhive attach wh@id@proj@feature-x   # Attach via full session name
  workhive attach                        # Interactive: select from active sessions
  workhive attach main -r                # View main in read-only mode
  workhive attach main --force-layout    # Rebuild layout with latest config

Use cases:
  • SSH reconnect: Re-enter a still-running session after reconnect
  • After detach: Ctrl+B d to detach, then re-enter
  • After config change: Rebuild layout with --force-layout
  • Multi-terminal: View same workspace in multiple terminal windows

Notes:
  - If target session does not exist, attach fails and prints available sessions
  - Use 'workhive open <worktree>' to create a new workspace/session
  - List active sessions: workhive list --sessions-only
  - If already in tmux, will switch client instead of nesting
  - Read-only mode requires tmux 3.0+
EOF
}

_attach_command_help_zh() {
    cat <<EOF
用法: workhive attach [session|branch] [options]

附加到已存在的 tmux 会话。该命令不会创建 Worktree 或会话。

参数:
  <session|branch>      已存在的 tmux 会话名，或对应分支快捷名
                        如果省略,弹出交互式选择菜单

选项:
  -r, --read-only       以只读模式进入 (仅查看,不可操作)
  --force-layout        强制按配置文件重建布局 (修改 config.yaml 后使用)
  -h, --help            显示帮助信息

示例:
  workhive attach feature-x              # 通过分支快捷名附加
  workhive attach wh@id@proj@feature-x   # 通过完整会话名附加
  workhive attach                        # 交互式: 从活跃会话中选择
  workhive attach main -r                # 以只读模式查看 main
  workhive attach main --force-layout    # 按最新配置重建布局

使用场景:
  • SSH 重连: 重连后重新进入仍在运行的会话
  • 分离后重进: Ctrl+B d 分离后重新进入
  • 多终端协作: 在多个终端窗口中查看同一个工作空间
  • 修改配置后: 使用 --force-layout 立即应用新布局

注意:
  - 如果目标会话不存在, attach 会失败并列出可用会话
  - 创建新空间请使用: workhive open <worktree>
  - 查看活跃会话: workhive list --sessions-only
  - 如果已在 tmux 中,将自动切换而非嵌套
  - 只读模式需要 tmux 3.0+
EOF
}

close_command_help() {
    _help_dispatch "_close_command_help"
}

_close_command_help_en() {
    cat <<EOF
Usage: workhive close [options]

Leave the current workhive workspace from inside a tmux pane.

Default behavior (no flags): interactive menu to choose between
detach (preserve session) or kill (terminate session).

Options:
  --force, -f           Detach immediately without interactive menu
  --kill                Terminate the tmux session (with confirmation)
  --force --kill        Terminate the tmux session without confirmation
  -h, --help            Show help

Examples:
  workhive close                # Interactive: choose detach or kill
  workhive close --force        # Detach immediately (session preserved)
  workhive close --kill         # Kill session (with confirmation)
  workhive close --force --kill # Kill session without confirmation

Notes:
  - Can ONLY be used inside a tmux pane (i.e. inside a workhive workspace)
  - Detach preserves the session; re-enter with: workhive attach <branch>
  - Kill terminates the session but does NOT remove the worktree or branch
  - To fully remove a workspace, use: workhive rm <branch>
EOF
}

_close_command_help_zh() {
    cat <<EOF
用法: workhive close [options]

从 tmux pane 内离开当前 workhive 工作空间。

默认行为 (无参数): 交互式菜单选择 detach (保留会话) 或 kill (终止会话)。

选项:
  --force, -f           立即 detach，跳过交互菜单
  --kill                终止 tmux 会话 (需确认)
  --force --kill        终止 tmux 会话，跳过确认
  -h, --help            显示帮助信息

示例:
  workhive close                # 交互式: 选择 detach 或 kill
  workhive close --force        # 立即 detach (会话保留)
  workhive close --kill         # 终止会话 (需确认)
  workhive close --force --kill # 终止会话，跳过确认

注意:
  - 只能在 tmux pane 内使用 (即在 workhive 工作空间内部)
  - Detach 保留会话; 重新进入: workhive attach <branch>
  - Kill 终止会话，但不会删除 worktree 或分支
  - 彻底删除工作空间请使用: workhive rm <branch>
EOF
}

list_command_help() {
    _help_dispatch "_list_command_help"
}

_list_command_help_en() {
    cat <<EOF
Usage: workhive list [options]
       workhive ls [options]

Global dashboard: show comprehensive status of all Worktree workspaces

Options:
  --format <format>     Output format: table (default) | json
  --layout              Show detailed layout configuration
  --watch               Watch mode: refresh status periodically
  --interval <seconds>  Refresh interval in watch mode (default: 5)
  -h, --help            Show help

Notes:
  - --layout cannot be combined with --format/--watch
  - --watch cannot be combined with --format json

Examples:
  workhive list                    # Table format dashboard
  workhive ls --format json        # JSON output for scripting
  workhive list --layout           # Show layout details

Output columns:
  • Branch: Git branch name
  • Worktree: Whether Worktree directory exists
  • Session: Session summary (active / detached / mixed / none)
  • Git Status: Working tree status (clean / modified / untracked)
  • Last Commit: Time since latest commit
  • Session Details: Separate section (branch, worktree, session, state, layout, recent session)
EOF
}

_list_command_help_zh() {
    cat <<EOF
用法: workhive list [options]
      workhive ls [options]

全局仪表盘: 显示所有 Worktree 工作空间的综合状态

选项:
  --format <format>     输出格式: table (默认) | json
  --layout              显示布局配置详情
  --watch               监视模式: 定期刷新状态
  --interval <seconds>  监视模式下的刷新间隔秒数 (默认: 5)
  -h, --help            显示帮助信息

注意:
  - --layout 不能与 --format/--watch 同时使用
  - --watch 不能与 --format json 同时使用

示例:
  workhive list                    # 表格格式仪表盘
  workhive ls --format json        # JSON 格式 (脚本使用)
  workhive list --layout           # 查看布局配置详情

输出说明:
  • 分支: Git 分支名
  • Worktree: Worktree 目录是否存在
  • 会话: 会话汇总状态 (活跃 / 断开 / 混合 / 无)
  • Git 状态: 工作区状态 (clean / modified / untracked)
  • 最近提交: 距离最近一次提交的时间
  • 会话详情: 独立区块展示分支、Worktree、会话、状态、布局、最近会话
EOF
}

remove_command_help() {
    _help_dispatch "_remove_command_help"
}

_remove_command_help_en() {
    cat <<EOF
Usage: workhive remove <worktree> [options]
       workhive rm <worktree> [options]

Destroy Worktree completely (tmux session + Worktree code + Git branch)

Arguments:
  <worktree>            Worktree name (same as used with open)

Options:
  --keep-worktree       Keep Worktree code and branch (close tmux session only)
  --remove-worktree     Remove Worktree code and branch (default)
  --force, -f           Force delete (discard uncommitted changes)
  -h, --help            Show help

Examples:
  workhive remove feature-x                  # Full destroy (session + code + Worktree)
  workhive remove bugfix-123 --keep-worktree # Close session only, keep Worktree
  workhive remove old-feature --force        # Force delete (discard uncommitted changes)

Behavior:
  • Default: fully remove Worktree code and Git branch (worktree.auto_delete: true)
  • If Worktree has uncommitted changes, confirmation will be prompted
  • tmux session is always closed
  • Force mode skips all confirmations

Notes:
  - Default removes Git branch as well (full cleanup)
  - Uncommitted code cannot be recovered after delete
  - Recommend commit or push changes before remove
EOF
}

_remove_command_help_zh() {
    cat <<EOF
用法: workhive remove <worktree> [options]
      workhive rm <worktree> [options]

彻底销毁Worktree (tmux 会话 + Worktree 代码 + Git 分支)

参数:
  <worktree>            Worktree 名称 (即 open 时使用的名称)

选项:
  --keep-worktree       保留 Worktree 代码和分支 (仅关闭 tmux 会话)
  --remove-worktree     删除 Worktree 代码及分支 (默认行为)
  --force, -f           强制删除 (丢弃未提交更改)
  -h, --help            显示帮助信息

示例:
  workhive remove feature-x                  # 彻底销毁 (删除会话 + 代码 + Worktree)
  workhive remove bugfix-123 --keep-worktree # 仅关闭会话,保留Worktree
  workhive remove old-feature --force        # 强制删除 (丢弃未提交更改)

行为说明:
  • 默认彻底删除 Worktree 代码及关联 Git 分支 (worktree.auto_delete: true)
  • 如果 Worktree 有未提交更改,会提示确认
  • tmux 会话总是会被关闭
  • 强制模式会跳过所有确认

注意:
  - 默认会连同 Git 分支一并删除 (彻底清理)
  - 删除后未提交的代码不可恢复
  - 建议先 commit 或 push 更改再执行 remove
EOF
}

rename_command_help() {
    _help_dispatch "_rename_command_help"
}

_rename_command_help_en() {
    cat <<EOF
Usage: workhive rename <old_branch> <new_branch> [options]

Rename a workhive workspace (worktree + tmux session + state files).

Arguments:
  <old_branch>         Current branch/worktree name
  <new_branch>        New branch/worktree name

Options:
  --force, -f         Force rename even if session is attached
  -h, --help          Show help

Steps performed:
  1. Rename Git branch
  2. Move worktree directory
  3. Rename tmux session
  4. Rename layout state files

Notes:
  - Cannot rename the main branch
  - Old branch must exist; new branch must not exist
  - Rollback on failure (transaction pattern)
EOF
}

_rename_command_help_zh() {
    cat <<EOF
用法: workhive rename <旧分支> <新分支> [选项]

重命名 workhive 工作空间 (worktree + tmux 会话 + 状态文件)。

参数:
  <old_branch>         当前分支/worktree 名称
  <new_branch>         新分支/worktree 名称

选项:
  --force, -f          即使会话已附加也强制重命名
  -h, --help           显示帮助

执行步骤:
  1. 重命名 Git 分支
  2. 移动 worktree 目录
  3. 重命名 tmux 会话
  4. 重命名布局状态文件

注意:
  - 不能重命名主分支
  - 旧分支必须存在；新分支必须不存在
  - 失败时自动回滚 (事务模式)
EOF
}

pane_command_help() {
    _help_dispatch "_pane_command_help"
}

_pane_command_help_en() {
    cat <<EOF
Usage: workhive pane <subcommand> [options]

Manage panes in the current tmux session.

Subcommands:
  open [worktree]       Open a tool in the current pane for a Worktree
                        If Worktree does not exist, prompts to create it
                        If omitted, interactive selection menu appears
  close [-f|--force]    Close the current pane (siblings auto-fill space)
  split [--cmd <cmd>]   Split current pane below (preserves left/right structure)
  send <pane> <text>    Send text to a specific pane (for AI automation)
  list                  List all panes in current session
  current               Show current pane details

Options:
  -h, --help            Show help

Examples:
  workhive pane open                   # Interactive: select Worktree → select command
  workhive pane open dev01             # Open tool in dev01 (create if not exists)
  workhive pane open dev01 --cmd claude  # Open dev01 with claude (non-interactive)
  workhive pane close                  # Close current pane (with confirmation)
  workhive pane close -f               # Close current pane (skip confirmation)
  workhive pane split                  # Split below (new shell)
  workhive pane split --cmd lazygit    # Split below and run lazygit
  workhive pane send 1 "npm test"      # Send "npm test" + Enter to pane 1
  workhive pane list                   # List panes in current session
  workhive pane current                # Show details of current pane

Notes:
  - Must be run inside a tmux session pane
  - open: selected command replaces current shell (exec)
  - open --cmd: skips interactive menu, uses specified command directly
  - close: cannot close the last pane in a session
  - close -f: skips confirmation, suitable for AI automation
  - split: creates a new pane below, preserving left/right structure
  - send: uses tmux send-keys -l (literal mode), safe for CJK text
EOF
}

_pane_command_help_zh() {
    cat <<EOF
用法: workhive pane <子命令> [options]

管理当前 tmux 会话中的 pane。

子命令:
  open [worktree]       在当前 pane 中为 Worktree 打开工具
                        如果 Worktree 不存在,提示用户确认创建
                        如果省略,弹出交互式选择菜单
  close [-f|--force]    关闭当前 pane (相邻 pane 自动填充空间)
  split [--cmd <cmd>]   在当前 pane 下方分割 (保持左右布局)
  send <pane> <text>    向指定 pane 发送文本 (用于 AI 自动化)
  list                  列出当前会话中的所有 pane
  current               查看当前 pane 详情

选项:
  -h, --help            显示帮助信息

示例:
  workhive pane open                   # 交互式: 选择 Worktree → 选择命令
  workhive pane open dev01             # 在 dev01 中打开工具 (不存在则创建)
  workhive pane open dev01 --cmd claude  # 在 dev01 中打开 claude (非交互式)
  workhive pane close                  # 关闭当前 pane (需确认)
  workhive pane close -f               # 关闭当前 pane (跳过确认)
  workhive pane split                  # 在下方分割新 pane
  workhive pane split --cmd lazygit    # 在下方分割并运行 lazygit
  workhive pane send 1 "npm test"      # 向 pane 1 发送 "npm test" + 回车
  workhive pane list                   # 列出当前会话的 pane
  workhive pane current                # 查看当前 pane 详情

注意:
  - 必须在 tmux 会话内的 pane 中执行
  - open: 选择的命令会替换当前 shell (exec)
  - open --cmd: 跳过交互式菜单, 直接使用指定命令
  - close: 不能关闭会话中的最后一个 pane
  - close -f: 跳过确认, 适合 AI 自动化场景
  - split: 在下方创建新 pane, 保持左右结构不变
  - send: 使用 tmux send-keys -l (literal 模式), 支持中文
EOF
}

config_command_help() {
    _help_dispatch "_config_command_help"
}

_config_command_help_en() {
    cat <<EOF
Usage: workhive config [options]
       workhive config set <key> <value> [--global]
       workhive config get <key> [--global]
       workhive config git-whitelist [add <path>|edit <old-path> <new-path>|remove <path>|list|manage]
       workhive config --layout [--global]

Interactive view and modify project configuration

Subcommands:
  set <key> <value>      Set a config value (dot-path key)
  get <key>              Get a config value (dot-path key)
  git-whitelist          Manage git init whitelist paths (global config)

Options:
  --edit                 Open config file directly in editor
  --global               View/modify global configuration
  --layout               Interactive layout editor (config only)
  -h, --help             Show help

Examples:
  workhive config              # Interactive view/modify project config
  workhive config --global     # Interactive view/modify global config
  workhive config --edit       # Edit project config file directly
  workhive config --global --edit  # Edit global config file directly
  workhive config set tmux.theme tokyo-night
  workhive config set tmux.persistence.enabled true --global
  workhive config get tmux.theme
  workhive config get tmux.persistence.enabled --global
  workhive config --layout
  workhive config --global --layout

Config items:
  • Layout mode and Pane commands
  • tmux theme
  • Session snapshot restore settings
  • Worktree auto-management settings
  • Hooks config
  • Tools registry (global-only, managed via: workhive tools ...)

Config files:
  Project: .workhive/config.yaml
  Global:  \$(config_get_global_file)

Priority: Project > Global > Built-in defaults
EOF
}

_config_command_help_zh() {
    cat <<EOF
用法: workhive config [options]
      workhive config set <key> <value> [--global]
      workhive config get <key> [--global]
      workhive config git-whitelist [add <path>|edit <旧路径> <新路径>|remove <path>|list|manage]
      workhive config --layout [--global]

交互式查看和修改项目配置

子命令:
  set <key> <value>      设置配置值 (点号分隔的键路径)
  get <key>              获取配置值 (点号分隔的键路径)
  git-whitelist          管理 git init 白名单路径 (全局配置)

选项:
  --edit                 直接使用编辑器打开配置文件
  --global               查看/修改全局配置
  --layout               交互式布局编辑（仅修改配置）
  -h, --help             显示帮助信息

示例:
  workhive config              # 交互式查看/修改项目配置
  workhive config --global     # 交互式查看/修改全局配置
  workhive config --edit       # 直接编辑项目配置文件
  workhive config --global --edit  # 直接编辑全局配置文件
  workhive config set tmux.theme tokyo-night
  workhive config set tmux.persistence.enabled true --global
  workhive config get tmux.theme
  workhive config get tmux.persistence.enabled --global
  workhive config --layout
  workhive config --global --layout

配置项:
  • 布局模式和 Pane 命令
  • tmux 主题
  • 会话快照恢复设置
  • Worktree 自动管理设置
  • Hooks 配置
  • tools 注册表 (仅全局，使用 workhive tools ... 管理)

配置文件:
  项目级: .workhive/config.yaml
  全局级: \$(config_get_global_file)

优先级: 项目配置 > 全局配置 > 内置默认值
EOF
}

tools_command_help() {
    _help_dispatch "_tools_command_help"
}

_tools_command_help_en() {
    cat <<EOF
Usage: workhive tools <subcommand> [options]

Manage global tools registry (~/.config/workhive/config.yaml).

Subcommands:
  list                          List configured tools
  add --name <n> --cmd <c>      Add a tool
  edit <name> [--label <l>] [--cmd <c>] [--ai <true|false>] [--enable <true|false>]
                                Edit a tool
  remove <name>                 Remove a tool
  set-default <name>            Set default tool
  validate                      Validate tools config
  manage                        Interactive manager
  -h, --help                    Show help

Notes:
  - tools are global-only; project-level tools are ignored
  - tool_name must be unique
EOF
}

_tools_command_help_zh() {
    cat <<EOF
用法: workhive tools <subcommand> [options]

管理全局工具注册表 (~/.config/workhive/config.yaml)。

子命令:
  list                          列出工具
  add --name <n> --cmd <c>      添加工具
  edit <name> [--label <l>] [--cmd <c>] [--ai <true|false>] [--enable <true|false>]
                                编辑工具
  remove <name>                 删除工具
  set-default <name>            设置默认工具
  validate                      校验工具配置
  manage                        交互式管理
  -h, --help                    显示帮助

说明:
  - tools 仅支持全局配置；项目级 tools 会被忽略
  - tool_name 必须唯一
EOF
}

doctor_command_help() {
    _help_dispatch "_doctor_command_help"
}

_doctor_command_help_en() {
    cat <<EOF
Usage: workhive doctor [options]

Check environment health and diagnose potential issues

Options:
  --full                Run extended checks (dependencies, snapshot restore)
  --format <format>     Output format: table (default) | json
  -h, --help            Show help

Examples:
  workhive doctor                  # Run health check
  workhive doctor --format json    # JSON output for CI/scripting

Checks:
  • Required dependencies (git, tmux, yq)
  • Pane tools (lazygit, claude, codex, opencode, yazi)
  • Config file validity
  • tmux version compatibility
  • Git repository status
EOF
}

_doctor_command_help_zh() {
    cat <<EOF
用法: workhive doctor [options]

检查环境健康状态并诊断潜在问题

选项:
  --full                运行扩展检查 (依赖、快照恢复)
  --format <format>     输出格式: table (默认) | json
  -h, --help            显示帮助信息

示例:
  workhive doctor                  # 运行健康检查
  workhive doctor --format json    # JSON 格式 (CI/脚本使用)

检查项:
  • 必需依赖 (git, tmux, yq)
  • Pane 工具 (lazygit, claude, codex, opencode, yazi)
  • 配置文件有效性
  • tmux 版本兼容性
  • Git 仓库状态
EOF
}

gc_command_help() {
    _help_dispatch "_gc_command_help"
}

_gc_command_help_en() {
    cat <<EOF
Usage: workhive gc [options]

Clean up worktrees for branches that have been merged into the main branch

Options:
  --auto                Auto-clean without confirmation
  --snapshots           Clean snapshot files only (.workhive/snapshots)
  -h, --help            Show help

Examples:
  workhive gc                # Interactive cleanup
  workhive gc --auto         # Auto cleanup merged branches
  workhive gc --snapshots    # Clean snapshot files only
  workhive gc --snapshots --auto

Cleans:
  • Worktrees whose branches have been merged
  • Worktrees with no changes (identical to parent branch)
  • Orphan sessions (sessions without a worktree)
  • Stale state files in .workhive/state/
EOF
}

_gc_command_help_zh() {
    cat <<EOF
用法: workhive gc [options]

清理已合并到主分支的 Worktree

选项:
  --auto                自动清理,不需确认
  --snapshots           仅清理快照文件 (.workhive/snapshots)
  -h, --help            显示帮助信息

示例:
  workhive gc                # 交互式清理
  workhive gc --auto         # 自动清理已合并分支
  workhive gc --snapshots    # 仅清理快照文件
  workhive gc --snapshots --auto

清理内容:
  • 已合并分支对应的 Worktree
  • 无任何修改的 Worktree (与父分支完全一致)
  • 孤立会话 (没有对应 Worktree 的会话)
  • .workhive/state/ 中的过期状态文件
EOF
}

destroy_command_help() {
    _help_dispatch "_destroy_command_help"
}

_destroy_command_help_en() {
    cat <<EOF
Usage: workhive destroy [options]

Remove all workhive resources and reset to uninitialized state.

This command will:
  1. Close all workhive-managed tmux sessions
  2. Delete all Worktrees (code + Git branches)
  3. Delete .workhive/ config directory (config, logs, cache)

Options:
  --global              Delete global configuration only
  --force, -f           Skip all confirmation prompts
  -h, --help            Show help

Examples:
  workhive destroy                  # Interactive confirmation, then destroy
  workhive destroy --force          # Skip confirmation, destroy immediately
  workhive destroy --global         # Delete global config (~/.config/workhive/)
  workhive destroy --global --force # Delete global config without confirmation

Notes:
  - This operation is irreversible! All uncommitted code will be lost
  - Does not delete project code itself (only .workhive/ and Worktrees)
  - Does not delete .git directory
  - Run 'workhive init' to reinitialize after
  - --global only removes global config, not project-level resources
EOF
}

_destroy_command_help_zh() {
    cat <<EOF
用法: workhive destroy [options]

彻底清除 workhive 的所有资源,恢复到未初始化状态。

此命令会:
  1. 关闭所有 workhive 管理的 tmux 会话
  2. 删除所有 Worktree (代码 + Git 分支)
  3. 删除 .workhive/ 配置目录 (包括配置、日志、缓存)

选项:
  --global              仅删除全局配置
  --force, -f           跳过所有确认提示
  -h, --help            显示帮助信息

示例:
  workhive destroy                  # 交互式确认后彻底清除
  workhive destroy --force          # 跳过确认,直接清除
  workhive destroy --global         # Delete global configuration (~/.config/workhive/)
  workhive destroy --global --force # 无需确认直接删除全局配置

注意:
  - 此操作不可逆! 所有未提交的代码将丢失
  - 不会删除项目代码本身 (仅删除 .workhive/ 和 Worktree)
  - 不会删除 .git 目录
  - 执行后需重新 'workhive init' 初始化
  - --global 仅删除全局配置,不影响项目级资源
EOF
}

 upgrade_command_help() {
    _help_dispatch "_upgrade_command_help"
}

_upgrade_command_help_en() {
    cat <<EOF
Usage: workhive upgrade [options]

Update workhive to the latest version from GitHub.

Options:
  --check               Check for updates only (don't install)
  --version <tag>       Install specific version (e.g., v0.3.0)
  --force, -f           Force reinstall even if already up-to-date
  -h, --help            Show help

Examples:
  workhive upgrade                    # Upgrade to latest version
  workhive upgrade --check            # Check if update is available
  workhive upgrade --version v0.3.0   # Install specific version
EOF
}

_upgrade_command_help_zh() {
    cat <<EOF
用法: workhive upgrade [选项]

从 GitHub 获取最新版本并更新 workhive。

选项:
  --check               仅检查更新 (不安装)
  --version <tag>       安装指定版本 (如 v0.3.0)
  --force, -f           强制重新安装 (即使已是最新)
  -h, --help            显示帮助信息

示例:
  workhive upgrade                    # 升级到最新版本
  workhive upgrade --check            # 检查是否有更新
  workhive upgrade --version v0.3.0   # 安装指定版本
EOF
}

completions_command_help() {
    cat <<EOF
Usage: $APP_NAME completions <shell>

Output shell completion script to stdout.

Subcommands:
  <shell>    Output completion script (zsh, bash, fish)

Supported shells: zsh, bash, fish

Setup (manual):
  # Zsh — add to ~/.zshrc:
  eval "\$($APP_NAME completions zsh)"

  # Bash — add to ~/.bashrc (or ~/.bash_profile on macOS):
  eval "\$($APP_NAME completions bash)"

  # Fish — add to ~/.config/fish/config.fish:
  $APP_NAME completions fish | source

Recommended setup:
  $APP_NAME shell setup --profile minimal --enable completions
EOF
}

log_command_help() {
    _help_dispatch "_log_command_help"
}

_log_command_help_en() {
    cat <<EOF
Usage: workhive log [options]

View workhive operation logs

Options:
  --follow              Tail -f style live log watching
  --lines <N>           Show last N lines (default: 50)
  --clear               Truncate log file (clear contents)
  --level <level>       Filter by log level: debug | info | warn | error
  -h, --help            Show help

Examples:
  workhive log                    # Show last 50 lines
  workhive log --follow           # Live tail
  workhive log --lines 100        # Show last 100 lines
  workhive log --level error      # Show only ERROR entries
  workhive log --clear            # Clear log file
EOF
}

_log_command_help_zh() {
    cat <<EOF
用法: workhive log [options]

查看 workhive 操作日志

选项:
  --follow              实时跟踪日志 (类似 tail -f)
  --lines <N>           显示最后 N 行 (默认: 50)
  --clear               清空日志文件
  --level <level>       按级别过滤: debug | info | warn | error
  -h, --help            显示帮助信息

示例:
  workhive log                    # 显示最后 50 行
  workhive log --follow           # 实时跟踪
  workhive log --lines 100        # 显示最后 100 行
  workhive log --level error      # 仅显示 ERROR 级别
  workhive log --clear            # 清空日志
EOF
}

shell_command_help() {
    _help_dispatch "_shell_command_help"
}

_shell_command_help_en() {
    cat <<EOF
Usage: workhive shell <subcommand> [options]

Manage workhive shell integration in one managed entrypoint.

Subcommands:
  setup [--shell <shell>]  Interactive/flag-based setup (profile + modules)
  init [--shell <shell>]   Alias of setup
  remove [--shell <shell>] Remove shell integration from rc file
  show                     Print the integration script to stdout

Options:
  --shell <shell>     Target shell: zsh, bash (default: auto-detect)
  --profile <name>    Profile: minimal | standard | advanced
  --enable <csv>      Enable modules (completions,aliases,history,zoxide,starship,ssh-term-fix)
                       When provided, skips module-by-module interactive customization
  --disable <csv>     Disable modules from selected profile
                       When provided, skips module-by-module interactive customization
  --yes               Non-interactive mode (defaults profile=minimal)
  --dry-run           Show what would be done without making changes
  -h, --help          Show help

Examples:
  workhive shell setup                                 # Interactive setup
  workhive shell setup --shell zsh --profile minimal  # Minimal zsh setup
  workhive shell setup --profile standard --enable zoxide
  workhive shell init --shell zsh                      # Alias of setup
  workhive shell remove                # Remove integration
  workhive shell show                  # Print integration script
EOF
}

_shell_command_help_zh() {
    cat <<EOF
用法: workhive shell <subcommand> [options]

统一管理 workhive shell 集成（单入口托管）

子命令:
  setup [--shell <shell>]  交互式/参数式配置（profile + 模块）
  init [--shell <shell>]   setup 的别名
  remove [--shell <shell>] 从配置文件移除 shell 集成
  show                     输出集成脚本到标准输出

选项:
  --shell <shell>     目标 shell: zsh, bash (默认: 自动检测)
  --profile <name>    配置档位: minimal | standard | advanced
  --enable <csv>      启用模块 (completions,aliases,history,zoxide,starship,ssh-term-fix)
                       传入后会跳过“逐项自定义模块”交互
  --disable <csv>     关闭 profile 中的指定模块
                       传入后会跳过“逐项自定义模块”交互
  --yes               非交互模式 (默认 profile=minimal)
  --dry-run           预览更改 (不实际修改)
  -h, --help          显示帮助

示例:
  workhive shell setup                                # 交互式配置
  workhive shell setup --shell zsh --profile minimal # 最小化 zsh 配置
  workhive shell setup --profile standard --enable zoxide
  workhive shell init --shell zsh                     # setup 别名
  workhive shell remove                # 移除集成
  workhive shell show                  # 输出集成脚本
EOF
}

# ──────────────────────────────────────────────────
# Pane subcommand help functions
# ──────────────────────────────────────────────────

_pane_open_help_en() {
    cat <<EOF
Usage: workhive pane open [worktree] [options]

Open a tool in the current pane for a Worktree.
If Worktree does not exist, prompts to create it.
If no worktree is specified, interactive selection menu appears.

Options:
  --cmd <command>       Specify command directly (skip interactive menu)
                        Aliases: claude, codex, opencode, lazygit, yazi, shell
  -f, --force           Auto-create worktree if it does not exist (skip confirmation)
  -h, --help            Show this help

Examples:
  workhive pane open                         # Interactive: select Worktree → select command
  workhive pane open dev01                   # Open tool in dev01 (create if not exists)
  workhive pane open dev01 --cmd claude      # Open claude directly (non-interactive)
  workhive pane open feature-x --cmd claude -f  # Auto-create worktree + open claude

Notes:
  - Must be run inside a tmux session pane
  - The selected command replaces the current shell (exec)
  - --cmd claude resolves to: claude --dangerously-skip-permissions
EOF
}
_pane_open_help_zh() {
    cat <<EOF
用法: workhive pane open [worktree] [选项]

在当前窗格中为 Worktree 打开工具。
若 Worktree 不存在，会提示创建。
未指定 worktree 时显示交互选择菜单。

选项:
  --cmd <command>       直接指定命令 (跳过交互菜单)
                        别名: claude, codex, opencode, lazygit, yazi, shell
  -f, --force           若 worktree 不存在则自动创建 (跳过确认)
  -h, --help            显示帮助

示例:
  workhive pane open                         # 交互: 选择 Worktree → 选择命令
  workhive pane open dev01                   # 在 dev01 中打开工具 (不存在则创建)
  workhive pane open dev01 --cmd claude      # 直接打开 claude (非交互)
  workhive pane open feature-x --cmd claude -f  # 自动创建 worktree + 打开 claude

说明:
  - 必须在 tmux 会话窗格内运行
  - 所选命令会替换当前 shell (exec)
  - --cmd claude 解析为: claude --dangerously-skip-permissions
EOF
}

_pane_close_help_en() {
    cat <<EOF
Usage: workhive pane close [options] [pane_index]

Close the current pane or a specific pane. Siblings auto-fill the vacated space.
Cannot close the last pane in a session.

Options:
  --id <pane_id>        Close a pane by tmux pane id (for example: %3)
  -f, --force           Skip confirmation prompt
  -h, --help            Show this help

Examples:
  workhive pane close                # Close current pane with confirmation
  workhive pane close 2              # Close pane index 2
  workhive pane close --id %3        # Close pane with tmux pane id %3
  workhive pane close -f             # Close immediately (skip confirmation)
  workhive pane close --force        # Same as -f

Notes:
  - Cannot close the last pane in a session (use 'workhive rm' instead)
  - Use -f for AI automation workflows
EOF
}
_pane_close_help_zh() {
    cat <<EOF
用法: workhive pane close [选项] [pane_index]

关闭当前窗格或指定窗格。相邻窗格会自动填充空出的空间。
不能关闭会话中的最后一个窗格。

选项:
  --id <pane_id>        通过 tmux pane id 关闭窗格 (例如: %3)
  -f, --force           跳过确认提示
  -h, --help            显示帮助

示例:
  workhive pane close                # 关闭当前窗格 (带确认)
  workhive pane close 2              # 关闭索引为 2 的窗格
  workhive pane close --id %3        # 关闭 tmux pane id 为 %3 的窗格
  workhive pane close -f             # 直接关闭 (跳过确认)
  workhive pane close --force        # 同 -f

说明:
  - 不能关闭会话中的最后一个窗格 (请用 'workhive rm')
  - 使用 -f 可用于 AI 自动化工作流
EOF
}

_pane_split_help_en() {
    cat <<EOF
Usage: workhive pane split [options]

Split the current pane below (vertical split), preserving left/right layout structure.

Options:
  --cmd <command>       Command to run in the new pane
  --size <percentage>   Split percentage (default: 50)
  -h, --help            Show this help

Examples:
  workhive pane split                        # New shell below
  workhive pane split --cmd lazygit          # New pane running lazygit
  workhive pane split --cmd "npm run dev"    # New pane running dev server
  workhive pane split --size 30              # New pane takes 30% height
  workhive pane split --cmd "workhive pane open task-a --cmd claude -f"
                                             # Split + open AI tool for a task branch
EOF
}
_pane_split_help_zh() {
    cat <<EOF
用法: workhive pane split [选项]

在当前窗格下方分割 (垂直分割)，保留左右布局结构。

选项:
  --cmd <command>       在新窗格中运行的命令
  --size <percentage>   分割百分比 (默认: 50)
  -h, --help            显示帮助

示例:
  workhive pane split                        # 新 shell
  workhive pane split --cmd lazygit          # 新窗格运行 lazygit
  workhive pane split --cmd "npm run dev"    # 新窗格运行开发服务器
  workhive pane split --size 30              # 新窗格占 30% 高度
  workhive pane split --cmd "workhive pane open task-a --cmd claude -f"
                                             # 分割 + 为任务分支打开 AI 工具
EOF
}

_pane_send_help_en() {
    cat <<EOF
Usage: workhive pane send <pane_index|idx1,idx2,...> <text> [options]

Send text to a specific pane. By default, appends Enter to submit the text.
Uses tmux send-keys -l (literal mode), safe for CJK characters and special symbols.
Use --all to broadcast the same text to every pane in the current session.

Arguments:
  <pane_index|...>      Target pane index, or comma-separated indices (e.g. 1,2,3)
  <text>                Text to send

Options:
  --all                 Broadcast text to all panes in current session
  --mode <mode>         Injection mode: auto | plain | paste | bracketed-paste
  --commit <mode>       Commit key: auto | enter | c-m | none
  --verbose             Print resolved strategy per target pane
  --role <role>         Resolve target pane by configured role (e.g. primary, review)
  --diagnose            Diagnose target pane state only (read-only, no key sending)
  -h, --help            Show this help

Examples:
  workhive pane send 1 "npm test"            # Send "npm test" + Enter to pane 1
  workhive pane send 2 "git status"          # Send "git status" + Enter to pane 2
  workhive pane send 1 "hello" --commit none # Send "hello" without Enter
  workhive pane send 3 "multi-line task" --mode auto --commit auto
                                             # Auto-adapt mode/key and cache by pane target
  workhive pane send 3 "draft prompt" --commit none
                                             # Type only, do not submit
  workhive pane send 1,2,3 "npm test"        # Send to multiple panes
  workhive pane send --role primary "pwd"    # Send to pane resolved by role
  workhive pane send --all "npm test"        # Broadcast command to all panes
  workhive pane send 2 --diagnose            # Diagnose pane 2 send path only

Notes:
  - Use 'workhive pane list' to discover available pane indices
  - --all cannot be combined with pane index, --role, or --diagnose
  - --mode auto and --commit auto adapt per pane and cache by target key
  - Designed for AI automation: dispatch tasks to AI coding tools in other panes
EOF
}
_pane_send_help_zh() {
    cat <<EOF
用法: workhive pane send <窗格索引|索引1,索引2,...> <文本> [选项]

向指定窗格发送文本。默认在末尾附加回车键以提交。
使用 tmux send-keys -l (字面模式)，安全支持中日韩字符和特殊符号。
使用 --all 可将相同文本广播到当前会话中的全部窗格。

参数:
  <窗格索引|...>        目标窗格索引，或逗号分隔多个索引 (例如 1,2,3)
  <文本>                要发送的文本

选项:
  --all                 将文本广播到当前会话全部窗格
  --mode <模式>         注入模式: auto | plain | paste | bracketed-paste
  --commit <模式>       提交按键: auto | enter | c-m | none
  --verbose             输出每个目标 pane 实际命中的发送策略
  --role <角色>         通过布局角色定位目标窗格 (如 primary、review)
  --diagnose            仅诊断目标窗格状态 (只读，不发送按键)
  -h, --help            显示帮助

示例:
  workhive pane send 1 "npm test"            # 向窗格 1 发送 "npm test" + 回车
  workhive pane send 2 "git status"          # 向窗格 2 发送 "git status" + 回车
  workhive pane send 1 "hello" --commit none # 发送 "hello" 但不按回车
  workhive pane send 3 "multi-line task" --mode auto --commit auto
                                             # 自动适配发送模式与提交键，并按 pane 目标缓存
  workhive pane send 3 "draft prompt" --commit none
                                             # 仅输入，不提交执行
  workhive pane send 1,2,3 "npm test"        # 同时发送到多个窗格
  workhive pane send --role primary "pwd"    # 按角色定位并发送
  workhive pane send --all "npm test"        # 向全部窗格广播命令
  workhive pane send 2 --diagnose            # 仅诊断窗格 2 的发送链路

说明:
  - 使用 'workhive pane list' 查看可用窗格索引
  - --all 不能与窗格索引、--role、--diagnose 组合使用
  - --mode auto 与 --commit auto 会按 pane 目标自动适配并缓存策略
  - 专为 AI 自动化设计: 向其他窗格中的 AI 编程工具分发任务
EOF
}

_pane_list_help_en() {
    cat <<EOF
Usage: workhive pane list [options]

List all panes in the current tmux session with their status.
Shows: index, pane_id, running command, working directory, size, status, AI agent, and agent status.

Options:
  --format <format>     Output format: table (default) | json
  --deep                Enable deep runtime probe (slower, richer diagnostics)
  -h, --help            Show this help

Output columns:
  Index       Pane index (use with 'pane send')
  Command     Currently running command (e.g. claude, zsh, lazygit)
  Directory   Working directory basename
  Size        Pane dimensions (width x height)
  Status      Active or dead indicator
  Agent       Detected AI agent name (if any)
  Agent Status
              running | waiting | idle | unknown

JSON fields (per pane):
  agent_status            Unified state: running|waiting_input|idle|unknown
  agent_state_source      protocol|heuristic|probe
  agent_state_confidence  high|medium|low
  agent_state_reason      Detection reason code
EOF
}
_pane_list_help_zh() {
    cat <<EOF
用法: workhive pane list [选项]

列出当前 tmux 会话中的所有窗格及其状态。
显示: 索引、pane_id、运行命令、工作目录、尺寸、状态、AI Agent 和 Agent 状态。

选项:
  --format <format>     输出格式: table (默认) | json
  --deep                启用深度运行态探测（较慢，诊断更完整）
  -h, --help            显示帮助

输出列:
  Index       窗格索引 (配合 'pane send' 使用)
  Command     当前运行的命令 (如 claude, zsh, lazygit)
  Directory   工作目录名称
  Size        窗格尺寸 (宽 x 高)
  Status      活跃或终止状态
  Agent       识别到的 AI Agent 名称 (如有)
  Agent 状态  running | waiting | idle | unknown

JSON 字段 (每个 pane):
  agent_status            统一状态: running|waiting_input|idle|unknown
  agent_state_source      protocol|heuristic|probe
  agent_state_confidence  high|medium|low
  agent_state_reason      探测原因码
EOF
}

_pane_current_help_en() {
    cat <<EOF
Usage: workhive pane current [options]

Show detailed information of the current pane in the tmux session.
Shows: index, pane_id, command, directory, full path, size, status, AI agent, and agent status metadata.

Options:
  --format <format>     Output format: table (default) | json
  --deep                Enable deep runtime probe (slower, richer diagnostics)
  -h, --help            Show this help
EOF
}

_pane_current_help_zh() {
    cat <<EOF
用法: workhive pane current [选项]

显示当前 tmux 会话中当前 pane 的详细信息。
显示: 索引、pane_id、命令、目录、完整路径、尺寸、状态、AI Agent 及其状态元数据。

选项:
  --format <format>     输出格式: table (默认) | json
  --deep                启用深度运行态探测（较慢，诊断更完整）
  -h, --help            显示帮助
EOF
}
