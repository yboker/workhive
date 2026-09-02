# workhive

[English](README.md)

workhive 是一个使用 Rust 构建的 tmux 开发工作区 CLI。它把 git worktree、tmux 会话、窗格布局恢复和项目配置组合成一个项目级工作区工作流。

## 为什么选择 workhive？

同时处理多个功能通常需要在 git 分支、终端窗口和手工搭建的窗格布局之间来回切换。workhive 让这些状态变得明确、可重复：

- **项目级工作区**：`workhive workspace open feature-auth` 会为一个分支创建或恢复隔离的工作区。
- **tmux 运行时管理**：每个工作区都有可预测的 tmux 会话和窗格布局。
- **可选快照恢复**：启用后会自动保存窗格布局变化，并在 `workspace open` 时恢复。
- **可信项目目录**：全局安全策略限制 Workhive 可以在哪里初始化项目、创建 worktree 和写入运行时状态。
- **运行时控制平面**：窗格通信通过类型化本地 supervisor，而不是继续堆叠临时 `tmux send-keys` 调用。
- **全局可见性**：项目列表和会话列表可以查看已注册项目和活跃会话，不需要记忆 tmux 名称。

## 安装

workhive 以单一二进制文件分发；推荐直接安装预编译产物——无需 Rust 工具链。

支持的运行平台是 macOS 和 Linux。Windows 用户应在 WSL2 中运行 workhive，并在
WSL2 里安装 Linux 版 `git` 和 `tmux`；原生 Windows、PowerShell、CMD 和 Git for
Windows 不是受支持的运行环境。

### 安装预编译二进制（推荐）

下载适用于你平台的 `workhive` 二进制文件，然后放入 `PATH`：

```bash
install -m 755 workhive ~/.local/bin/workhive   # 或 /usr/local/bin
workhive --version
```

### 从源码构建（贡献者）

需要 Rust stable（MSRV 1.95.0+）：

```bash
cargo build --release
install -m 755 target/release/workhive ~/.local/bin/workhive
```

## 快速开始

```bash
# 初始化全局配置
workhive init --global

# 在 git 仓库内初始化项目
cd ~/my-project
workhive init

# 打开或创建工作区
workhive workspace open feature-auth

# 列出当前项目的工作区
workhive workspace list

# 附加到已有工作区
workhive workspace attach feature-auth

# 在 workhive 窗格内关闭当前工作区
workhive workspace close
```

如果项目还没有初始化，`workhive workspace open` 会在交互模式下引导初始化。非交互环境需要传入 `--init`，显式允许项目初始化。

## 命令模型

workhive 将系统命令放在顶层，将资源操作按资源名称分组。

| 作用域 | 命令 |
|--------|------|
| 系统 | `workhive init`, `workhive config`, `workhive doctor`, `workhive completion` |
| 工作区 | `workhive workspace open`, `list`, `attach`, `close`, `remove`, `destroy` |
| 项目索引 | `workhive project list`, `reindex`, `roots` |
| 会话 | `workhive session list`, `attach` |
| 窗格 | `workhive pane split`, `close`, `list`, `show`, `send` |
| Worktree | `workhive workspace worktree list`, `remove` |

支持按对象限定的 `ls` 简写，例如 `workhive workspace ls`、
`workhive project ls`、`workhive session ls`、`workhive pane ls` 和
`workhive workspace worktree ls`。顶层 `workhive ls` 与 `workhive worktree`
不属于公开命令面。

### 工作区命令

| 命令 | 说明 |
|------|------|
| `workhive workspace open [workspace]` | 打开或创建当前项目内的工作区。不传名称时显示交互选择器。 |
| `workhive workspace open <workspace> --no-attach` | 创建或恢复工作区，但不附加到 tmux。 |
| `workhive workspace open <workspace> --mode manual\|primary-assisted` | 选择工作区交互模式，默认是 `manual`。 |
| `workhive workspace open <workspace> -f\|--force` | 从配置布局重新创建运行时状态，并丢弃已保存快照。 |
| `workhive workspace open <workspace> --init` | 项目未初始化时允许初始化。非交互自动初始化必须显式传入。 |
| `workhive workspace open <workspace> --attach-existing-branch` | 挂载一个已存在、但没有 Workhive 管理 worktree 的本地分支。不传该 flag 时，名称冲突会在交互终端要求确认，在非交互模式直接失败。 |
| `workhive workspace list [--view auto\|table\|compact\|json]` | 列出当前项目的工作区。默认视图会根据终端宽度自适应。 |
| `workhive workspace attach [workspace]` | 附加到已有工作区会话。不传名称时显示交互选择器。 |
| `workhive workspace close [--kill] [-f\|--force]` | 从当前工作区分离，或使用 `--kill` 终止运行时。必须在 workhive 窗格内执行。 |
| `workhive workspace remove <workspace> [--delete-branch] [--keep-branch] [-f\|--force]` | 移除一个受管理的工作区。Workhive 创建的分支默认删除；`--delete-branch` 授权删除用户自有分支；`--keep-branch` 无条件保留分支。 |
| `workhive workspace destroy [--delete-branch] [--keep-branch] [-f\|--force]` | 移除当前项目的全部 workhive 资源，包括工作区、运行时会话、快照和 `.workhive/`。分支 flag 语义与 `remove` 相同。 |

### 系统和资源命令

| 命令 | 说明 |
|------|------|
| `workhive init --global` | 使用交互式向导创建或更新全局配置。 |
| `workhive init` | 为当前项目初始化 workhive。 |
| `workhive config show [--global] [--raw]` | 显示项目或全局的生效配置。 |
| `workhive config explain <key>` | 解释配置键的可用值和语义。 |
| `workhive config edit [--global]` | 通过交互式向导编辑配置。 |
| `workhive config set <key> <value> [--global]` | 设置 TOML 配置值。 |
| `workhive config unset <key> [--global]` | 移除 TOML 配置值。 |
| `workhive config env-set <key> <value> [--global]` | 在 `.env` 中设置环境变量。 |
| `workhive config env-unset <key> [--global]` | 从 `.env` 中移除环境变量。 |
| `workhive doctor` | 检查 tmux、会话查找、快照、PTY 可用性和附加就绪状态。 |
| `workhive project list [--view auto\|table\|compact\|json]` | 从全局索引列出已注册项目。 |
| `workhive project reindex [--root <path>] [--config-roots] [--clear-first] [--dry-run] [-f\|--force]` | 重建全局项目索引。未显式指定根目录时使用已配置的可信目录。 |
| `workhive project roots list\|add\|remove\|edit\|status` | 管理全局配置中的可信项目目录。 |
| `workhive session list [--view auto\|table\|compact\|json]` | 跨已注册项目列出活跃工作区会话。 |
| `workhive session attach [target]` | 从全局附加到 Workhive 管理的会话。target 可以是会话名、`project/workspace` 或无歧义的工作区名。 |
| `workhive workspace worktree list` | 列出 `.workhive/worktrees/` 下 Workhive 管理的 linked worktree。别名：`ls`。 |
| `workhive workspace worktree remove [worktree] [--delete-branch] [--keep-branch] [--force]` | 移除受管理的 linked worktree。不传目标且是交互终端时显示选择器。分支 flag 语义与 `workspace remove` 相同。 |
| `workhive pane split [--direction right\|down\|left\|up] [--cmd <command>] [-f\|--force]` | 分割当前窗格，默认方向是 `down`。 |
| `workhive pane close [-f\|--force]` | 关闭当前窗格。 |
| `workhive pane list` | 列出当前会话内的窗格。 |
| `workhive pane show [pane]` | 显示当前窗格信息，或通过 `pane list` 编号（如 `2`）/ tmux id（如 `%2`）查看目标窗格。别名：`current`。 |
| `workhive pane send <pane> <text> [--no-enter] [-f\|--force]` | 向目标窗格发送文本。已验证的 AI 接收窗格会收到原文加结构化提交事件；`--no-enter` 只插入不提交。shell、TUI 和未验证窗格默认拒绝，必须用 `--force` 显式选择原始按键注入。 |

## Shell 自动补全

使用 `workhive completion install` 为当前用户安装自动补全。Workhive 会从
`$SHELL` 自动识别当前 shell；只有需要覆盖自动识别时才传 shell 名称。

支持的 shell：

```bash
workhive completion install
workhive completion install zsh
workhive completion install bash
workhive completion install fish
```

PowerShell 补全可用于 shell 集成，但这不代表 workhive 支持原生 Windows 运行环境。

安装位置：

| Shell | 安装文件 |
|-------|----------|
| bash | `~/.config/workhive/completions/bash/workhive` |
| zsh | `~/.config/workhive/completions/zsh/_workhive` |
| fish | `~/.config/workhive/completions/fish/workhive.fish` |
| powershell | `~/.config/workhive/completions/powershell/_workhive.ps1` |
| elvish | `~/.config/workhive/completions/elvish/workhive-completions.elv` |

安装后，打开新终端即可生效；如果要在当前终端立即生效，可以按 shell 执行：

| Shell | 当前终端生效方式 |
|-------|------------------|
| zsh | `source ~/.zshrc` |
| bash | 根据你的配置执行 `source ~/.bashrc` 或 `source ~/.bash_profile` |
| fish | `exec fish` |
| powershell | 如果 profile 中加载了 Workhive 补全，执行 `. $PROFILE` |
| elvish | 将已安装脚本加入 `rc.elv` 后重新加载 |

对于 zsh，`workhive completion install` 还会把 Workhive 管理的配置块幂等写入
`~/.zshrc`，让 zsh 从 Workhive 全局配置目录加载补全脚本。

补全覆盖命令、子命令、flag、`--view auto|table|compact|json` 这类枚举值，
以及 `workhive workspace ls` 这类按对象限定的简写。bash、zsh 和 fish 还会
在 Workhive 状态可用时动态补全工作区名称和窗格目标。

## 配置

配置使用 TOML。全局配置提供默认值，项目配置可以覆盖全局配置。

| 路径 | 用途 |
|------|------|
| `~/.config/workhive/config.toml` | 全局默认配置，包括语言、布局、tmux 命名、快照、日志、工具和发现路径。 |
| `~/.config/workhive/.env` | workhive 管理命令使用的全局环境变量。 |
| `~/.config/workhive/logs/` | 文件日志作用域为 `global` 时的全局日志目录。 |
| `~/.config/workhive/projects.json` | 全局项目注册表。 |
| `<project>/.workhive/config.toml` | `workhive init` 创建的可选项目配置。 |
| `<project>/.workhive/.env` | 项目环境变量。 |
| `<project>/.workhive/snapshots/` | 启用快照时的工作区布局快照。 |
| `<project>/.workhive/runtime/control-plane/` | 窗格运行时控制使用的本地 Unix socket supervisor 状态。 |
| `<project>/.workhive/runtime/pane-identity/` | 用于标题和窗格识别的持久化窗格启动身份。 |
| `<project>/.workhive/logs/` | 文件日志作用域为 `project` 时的项目日志目录。 |
| `<project>/.workhive/instance_id` | 用于 tmux 命名的项目实例标识。 |
| `<project>/.workhive/worktrees/` | workhive 管理的 git worktree 目录。 |

`workhive workspace open --init` 会创建打开工作区所需的运行时 `.workhive/` 结构，但不会创建 `config.toml`；项目配置由 `workhive init` 创建。

### 环境文件（`.env`）

`workhive workspace open`（以及快照恢复）会把生效的环境变量注入到每个由 workhive
启动的窗格进程：变量经由运行时端口流向 `tmux new-session -e KEY=VALUE` /
`tmux split-window -e KEY=VALUE`，因此目标程序直接继承这些变量，全程不会拼接 shell
字符串。

语法：固定使用 `KEY=VALUE`，不支持 YAML 写法：

```dotenv
# Workhive environment variables.
# Syntax: KEY=VALUE  (one per line; YAML-style "KEY: VALUE" is not accepted)
# Blank lines and lines starting with `#` are ignored.
#
# Global file: ~/.config/workhive/.env
# Project file: <project>/.workhive/.env
# Project entries override global entries with the same key.

# Claude Code 全屏渲染 / 减少闪烁。
# 详见: https://code.claude.com/docs/en/fullscreen#enable-fullscreen-rendering
# CLAUDE_CODE_NO_FLICKER=1
```

注意事项：

- 新建的 `.env` 文件由 `workhive init` 与 `workhive config env-set` 使用上面的
  自文档化模板生成。
- 错误的行（例如 `CLAUDE_CODE_NO_FLICKER: 1`）**不会**被静默忽略：workhive 会输出
  一条结构化警告，包含文件路径、行号以及类型化原因，文件其余部分仍会继续加载。
- `workspace open` 会向 stderr 打印一段已脱敏的环境变量摘要：注入来源、总数和
  key 名称。原始 value **永远不会**被打印或写入日志，即便 key 名看起来不像
  机密。
- 你可以直接编辑 `.env`，或使用 `workhive config env-set <key> <value>` /
  `workhive config env-unset <key>` 维护。这两个命令会保留标准模板头，且不会
  在日志中打印原始值。

### 快照

快照会把工作区的窗格布局保存在 `<project>/.workhive/snapshots/` 中，这样即使
tmux 会话被杀掉，下一次 `workhive workspace open <name>` 也能恢复之前的布局。
快照是可选功能。

- 初始化向导中"是否启用工作区快照?"的默认值已改为 **No**。已有用户配置不会被
  升级流程静默改写。
- 如需启用，请在 `~/.config/workhive/config.toml`（或项目的
  `.workhive/config.toml`）中设置 `[snapshot] enabled = true`；项目值会覆盖
  全局值。

### 可信项目目录（trusted project directories）

workhive 的可信目录策略决定了 `workhive init`、`workspace open --init`、
worktree 与运行时状态可以创建在哪些位置。首次运行向导会要求你做出明确选择：

- "是否现在配置可信项目目录?"必须明确回答 `Yes` 或 `No`。
- 选 **Yes** 进入交互式路径输入循环，每条路径都会被校验：
  - 必须是绝对路径（或以 `~` 开头），不接受相对路径；
  - 必须真实存在并且是目录；
  - 重复路径会被拒绝；
  - 明显过宽的路径（如 `/`、`~`、`$HOME`）会被拒绝，除非随后通过
    `project roots add --force` 强制添加。
- 路径输入框支持 Tab 补全：单一匹配自动补全，多匹配显示候选列表，无匹配则给出
  提示。
- 空输入**不**等同于"无限制"：向导会强制要求至少添加一条可信目录后才能继续。
- 选 **No** 表示当前不配置可信目录；之后可以通过
  `workhive project roots add <path>` 增量添加。

### 运行时控制平面

`workhive workspace open` 会为当前工作区启动一个 detached 本地 supervisor。
`workhive pane send` 这类窗格操作通过类型化 Unix socket 协议与 supervisor
通信。supervisor 会通过 ping 检查过期 socket，必要时进行一次自愈，并随工作区
生命周期退出。

这样可以把窗格通信、窗格状态和后续编排能力放在 Rust 领域边界之后。tmux 仍然是
当前适配器，但产品行为不再依赖调用方拼接原始 tmux 命令字符串。

### 窗格身份和标题

Workhive 会把受管理窗格的启动身份记录在
`<project>/.workhive/runtime/pane-identity/`。显示窗格信息以及 attach 前后的标题
重申都会使用这份身份，因此以 `codex`、`claude` 或其他配置工具启动的窗格不会退化
成 `shell` 这类泛化前台名称。

### AI 助手活动状态

Workhive 会观测 Claude Code、Codex、OpenCode 窗格正在做什么，并渲染每个窗格的
活动状态。**Workhive 不会安装 provider 侧的 hook 或 plugin。** 在
`init --global` 和 AI pane 启动时执行的自动准备流程是一个"清理 + notify"
生命周期：

- 清理早期版本遗留的 Workhive 自有 hook 条目、hook 脚本和 plugin 文件。用户
  自己编写的同路径条目会原样保留；任何对用户自有配置文件的改写之前都会先生成
  带时间戳的备份副本。
- 唯一保留的写入路径是可选的、由哨兵标记管理的 Codex `notify` 包装器（默认
  关闭，见下方 feature gate）。该包装器通过 Workhive 内部记录器记录完成事件，
  并把事件转发给用户已有的 `notify` 命令，保持失败开放（fail-open）。

活动信号改由 Workhive 侧观测提供：保守解析可见终端界面的终端状态探测、pane
supervisor 的前台进程观测、OS 进程观测以及 tmux 回退。hook/plugin 事件存在时
仍然是准确性最高的来源（例如用户自行安装、调用 Workhive 记录端点的 hook）。

**活动遥测保持失败开放（fail-open）。** 任何信号源缺失或失败只会让该窗格的
活动准确性下降，绝不会阻塞 Claude/Codex 的 prompt、tool call 或会话本身。

`workhive pane list` 把检测来源放在独立的 `来源` 列里（不再混入活动状态列，
窄屏下活动列保持简洁可读）：

| `来源` | 含义 |
|--------|------|
| `钩子` | Claude/Codex hook 事件报告了活动状态（最高准确性）。 |
| `插件` | OpenCode plugin 事件报告了活动状态（最高准确性）。 |
| `终端` | Workhive 终端状态探测从可见终端界面保守解析出状态（中等准确性）。 |
| `监控` | Workhive supervisor 看到 AI 工具在前台，但尚未收到更高准确性的信号（中等准确性）。 |
| `进程` | OS 进程观测是当前最强来源（中等准确性）。 |
| `tmux` | tmux 回退是当前唯一可用的信号（低准确性）。 |
| `无` | 暂无任何信号（低准确性）。 |

当存在较低准确性的来源时，`pane list` 会在表格下方追加一段保守说明 —— 来源
按准确性排序为 `完成信号 > 终端状态 > 进程 > tmux > 无` —— 并提示通过 Workhive
重启 AI 工具，让 Workhive 能直接观测新会话。

使用 `workhive pane show` 可以查看完整 AI 活动来源、准确性、是否过期以及改进建议。
如果准确性持续降级，`workhive doctor` 仍可检查 Workhive 通用环境。

#### AI 集成生命周期状态

每个 Workhive 管理的 AI provider 都走同一个类型化生命周期。状态属于辅助信息
（`pane show` 会渲染面向用户的投影），Workhive 不要求用户手动驱动这些转换。

| 状态 | 含义 |
|------|------|
| `Unsupported` | 该 provider 没有 Workhive 集成模型（hook/plugin 形态未知）。 |
| `ProviderUnavailable` | provider 可执行文件不在 `PATH` 上，本机暂无可安装内容。 |
| `NotInstalled` | provider 已安装，但 hook/plugin 尚未准备。 |
| `PrepareFailed` | 之前一次自动 prepare 遇到瞬时或语义错误；pane 生命周期保持失败开放，`pane show` 展示可操作的失败原因。 |
| `Installed` | Workhive hook/plugin 资产已在 provider 范围就位并保持当前。这是面向 provider 级诊断或验证的生命周期模型状态；`pane list` / `pane show` 的 pane 范围渲染不会出现该状态，会直接推进到 `PendingFirstEvent` / `Active` / `Stale`。 |
| `ReviewRequired` | hook/plugin 已落盘，但 provider 要求用户审核（Codex `/hooks`）。 |
| `PendingFirstEvent` | hook/plugin 已安装（并且按需已被批准），但当前 pane 尚未收到事件。 |
| `Active` | 该 pane 最近收到了 hook/plugin 事件。 |
| `Stale` | 最近一次 hook/plugin 事件已超过新鲜度窗口；活动列回退到较低置信来源。 |

#### Feature Gates

AI 集成生命周期 —— 清理/notify 准备流程、hook/plugin 活动事件接入、表格下方的
引导块 —— 由一个统一 feature gate 控制。该 gate **默认开启**。你可以在全局或
每个 provider 维度关闭它。**关闭后保持完全静默：**

- 准备流程不触碰该 provider 的任何配置文件；
- 不读取来自该 provider hook/plugin 的活动事件作为来源；
- `pane list` / `pane show` 不渲染任何与该 provider 相关的引导。

"被关闭"不是生命周期失败 —— `pane show` 在该 pane 上不会出现 AI 区块，活动列
按观测链路退化，就像其它普通终端窗格一样。

相关 gate：

- `[features.ai_integrations.providers.codex.notify]` —— 可选的 Codex
  `notify` 完成事件包装器。默认：`false`。
- `[features.pane_observation.terminal_status]` —— 终端状态探测。默认：
  `true`。

```toml
[features.ai_integrations]
enabled = true                    # 全局开关，默认 = true

[features.ai_integrations.providers.codex]
enabled = false                   # 关闭单个 provider

[features.ai_integrations.providers.codex.notify]
enabled = true                    # 选择启用 Codex notify 包装器

[features.pane_observation.terminal_status]
enabled = true                    # 终端状态探测，默认 = true
```

设 `[features.ai_integrations].enabled = false` 时所有 provider 一并关闭，
per-provider 覆盖此时被忽略。项目级配置直接继承全局配置，可以独立翻转任一
开关；没有需要记忆的"恢复全局"语义。

### 本地通知

Workhive 可以为 AI 窗格的状态转换发送本地桌面通知（macOS 通过系统通知守护
进程；不可用时降级为日志行，绝不阻塞 CLI）。投递策略保守：只考虑高置信、新鲜
的活动转换，每次投递都会去重，任何 prompt 或消息内容都不会进入通知。

Gate 写在 `[features.notifications]` 下，并提供分类别的噪声控制：

```toml
[features.notifications]
enabled = true                    # 总开关，默认 = true

[features.notifications.permission_needed]
enabled = true                    # 默认 = true

[features.notifications.failed]
enabled = true                    # 默认 = true

[features.notifications.completed]
enabled = false                   # 默认 = false（低噪声）

[features.notifications.all_panes_complete]
enabled = false                   # 默认 = false
```

### 配置示例

```toml
language = "zh-CN"

[tmux.naming]
server_template = "whsrv-{instance_id}-{project}"
session_template = "wh@{instance_id}@{project}@{workspace}"
max_component_len = 24

[layout]
template = "ai_coding_dual"

[[layout.columns]]
width = 35

[[layout.columns.panes]]
height = 100
launch = { type = "Tool", name = "lazygit" }

[[layout.columns]]
width = 65

[[layout.columns.panes]]
height = 50
launch = { type = "Tool", name = "codex" }

[[layout.columns.panes]]
height = 50
launch = { type = "EmptyShell" }

[snapshot]
# 向导默认关闭，需要时手动改为 `true` 即可启用快照。
enabled = false

[logging.console]
enabled = true
level = "warn"
format = "compact"

[logging.file]
enabled = true
level = "debug"
format = "compact"
scope = "global"
base_name = "workhive.log"
rotation = "size_mb(10)"
retention = "max_files(7)"
```

### 日志

控制台日志默认输出到 stderr，使用对人友好的 compact 格式。文件日志通过 `[logging.file]` 配置。

- `rotation = "size_mb(N)"` 表示活跃 `.log` 文件达到约 `N` MiB 后滚动。
- `retention = "max_files(N)"` 表示最多保留 `N` 个滚动后的日志文件；`unlimited` 表示不限制。
- `format = "compact"` 适合人工阅读；`json` 适合机器解析。
- 文件日志初始化失败不会阻塞 CLI 执行。

### 布局模板

内置模板包括 `ai_coding_left`、`ai_coding_dual`、`half_split`、`quad`、`single` 和 `custom`。模板会展开为显式的 `layout.columns` 配置。配置写入后，columns 是真正的布局来源。

### 窗格命令预设

初始化向导会检测已安装工具，并将其作为窗格启动预设：

| 工具 | 用途 |
|------|------|
| `claude` | Claude Code agent |
| `codex` | Codex agent |
| `opencode` | OpenCode agent |
| `lazygit` | Git TUI |
| `yazi` | 文件管理器 |
| `htop` | 系统监控 |
| `btm` | 系统监控 |

只有在 `$PATH` 中找到的工具才会显示。你也可以留空窗格或输入自定义命令。

## 架构

```text
workhive-cli
  clap 命令解析、miette 错误展示、tracing 日志、
  交互式向导和面向窗格的 CLI 渲染

workhive-core
  领域模型、用例、错误类型、i18n key 和外部端口

workhive-infra
  系统 git 和 tmux 适配器、TOML 配置适配器、快照存储、
  本地控制平面 supervisor、窗格启动身份存储、
  静态中英文目录、文件系统和项目索引适配器

workhive-testkit
  用于隔离测试的内存 fake 和 fixture
```

### 设计原则

- **六边形边界**：核心用例依赖端口，而不是具体 git、tmux、文件系统或配置实现。
- **枚举优先的 Rust 建模**：有限领域变体使用 enum 和穷举 `match`。
- **类型化错误**：core 和 infra 使用 `thiserror`；CLI 将错误映射为面向用户的 `miette` 报告和退出码。
- **CLI 国际化**：面向用户的消息通过 i18n key 解析。
- **可替换运行时适配器**：tmux 和系统 git 位于端口之后，未来可以引入新适配器而不重写用例。

### 关键端口

Core 端口包括 `GitPort`、`TmuxPort`、`ConfigPort`、`WorkspaceFsPort`、`ProjectIndexPort`、`ProjectDiscoveryPort`、`SnapshotStorePort`、`SnapshotRuntimePort`、`WorkspaceRuntimePort`、`PaneRuntimePort`、`PaneRegistryPort`、`PaneLaunchIdentityPort`、`DependencyPort`、`I18nPort` 和 `EnvironmentPort`。

## 会话命名

默认 tmux 命名规则：

- 会话：`wh@{instance_id}@{project}@{workspace}`
- 服务器：`whsrv-{instance_id}-{project}`

每个已初始化项目都会在 `.workhive/instance_id` 中保存 `instance_id`，用于保持 tmux 名称稳定并避免项目之间冲突。

## 系统要求

| 工具 | 必需 | 用途 |
|------|------|------|
| git | 是 | 仓库检测和 worktree 管理 |
| tmux | 是 | 终端会话和窗格 |
| Rust stable（MSRV 1.95.0+） | 仅构建时 | 从源码构建时需要 |

## 许可证

MIT OR Apache-2.0
