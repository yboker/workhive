# workhive

[English](README.md)

**面向 Git worktree 和终端运行时的工作区开盒器。**

Workhive 为项目准备一个可以直接工作的环境：Git worktree、运行时工作区，以及可选的
窗格布局。布局可以启动 Claude Code、Codex、OpenCode、lazygit 等命令。

Workhive 支持两个运行时后端：

- **tmux**：创建和管理 tmux server、session 以及 pane。
- **Herdr**：创建或采用 Git checkout，通过 Herdr API 注册 workspace，并管理其
  workspace、tab 和 pane。

## 安装

可以从 [GitHub Releases](https://github.com/yboker/workhive/releases) 下载预编译二进制。
Workhive 支持 macOS 和 Linux。Windows 用户请在 WSL2 中使用 Linux 版 `git` 和所选
运行时；原生 Windows、PowerShell、CMD 和 Git for Windows 不是支持的运行环境。

```bash
install -m 755 workhive ~/.local/bin/workhive
workhive --version
```

从源码构建需要 Rust stable（MSRV 1.95.0+）：

```bash
cargo build --release
install -m 755 target/release/workhive ~/.local/bin/workhive
```

## 快速开始

```bash
workhive init --global
cd ~/my-project
workhive init
workhive workspace open feature-auth
```

常用后续命令：

```bash
workhive workspace list
workhive workspace attach feature-auth
workhive workspace close # tmux 当前工作区
workhive workspace remove feature-auth
```

电脑重启后，可以在任意目录使用全局命令返回原生 Herdr 会话：

```bash
workhive session resume
workhive session resume workhive-managed
```

省略名称时，`session resume` 使用全局配置中的 `herdr.session`；未配置时使用
`workhive-managed`。会话和运行时的恢复由 Herdr 负责，本命令不会创建项目状态。项目级工作区的
创建、复用或显式重建仍使用 `workspace open`。

交互式终端中，`workspace open` 可以引导初始化项目。非交互调用需要显式传入 `--init`。

## 命令

运行 `workhive --help` 或 `<command> --help` 可以查看当前版本的完整帮助。

| 类别 | 命令 |
|------|------|
| 系统 | `init`、`config`、`doctor`、`completion` |
| 项目 | `project list`、`project reindex`、`project roots ...` |
| 工作区 | `workspace open`、`list`、`attach`、`close`、`remove`、`destroy` |
| 工作树 | `workspace worktree list`、`remove` |
| 会话 | `session list`、`attach`、`resume` |
| 窗格 | `pane split`、`close`、`list`、`show`、`send` |
| 运行时服务 | `runtime stop` |

`workspace`、`project`、`session`、`pane` 和 `workspace worktree` 支持对象范围内的
`ls` 别名。

### 工作区

```text
workspace open [workspace] [--no-attach] [--init] [--force]
                     [--mode manual|auto] [--attach-existing-branch]
workspace list [--view auto|table|compact|json]
workspace attach [workspace]
workspace close [--kill] [--force]
workspace remove <workspace> [--delete-branch|--keep-branch] [--force]
workspace destroy [--delete-branch|--keep-branch] [--force]
workspace worktree list
workspace worktree remove [worktree] [--delete-branch|--keep-branch] [--force]
```

`open` 创建或复用 Git worktree 和运行时工作区。不传名称时使用当前 Git 分支。
`--force` 根据生效配置重建选定的运行时。`remove` 和 `destroy` 只清理 Workhive
管理的资源；分支删除受保护，可以使用 `--keep-branch` 保留分支。

`workspace close` 用于 tmux 当前工作区的 detach/kill。Herdr 工作区根据需要使用
`workspace attach`、`session attach` 或 `workspace remove`。

### 配置、项目和运行时

```text
init [--global] [--backend tmux|herdr]
config show|explain|edit|set|unset|env-set|env-unset
doctor [--fix]
project list [--view auto|table|compact|json]
project reindex [--root <path>] [--config-roots] [--clear-first] [--dry-run] [--force]
project roots list|add|remove|edit|status
session list [--view auto|table|compact|json]
session attach [target]
session resume [session]
runtime stop [--force]
```

`project roots` 管理 Workhive 可以创建项目状态的可信目录。`doctor` 检查配置、后端
就绪状态、持久化绑定和接入条件。`runtime stop` 停止由 Workhive 托管的后端服务。

### 窗格

```text
pane split [--direction right|down|left|up] [--cmd <command>] [--force]
pane close [--force]
pane list [--view auto|table|compact|json]
pane show [pane]
pane send <pane> <text> [--no-enter] [--force]
```

两个后端都提供窗格操作，但目标和实现不同：

- tmux 使用 pane 编号或 tmux id（例如 `%2`）；向 shell 或 TUI 窗格发送原始输入时
  需要 `--force`。
- Herdr 使用 Herdr pane identity，并调用 Herdr 的 pane API。Herdr 布局支持右侧和向下
  的 split；接入时聚焦持久化 workspace，并使用 Herdr 的原生 session attach 路径。

`pane send` 向选定窗格发送文本。已验证的 AI 窗格通过运行时接收文本；shell、TUI 和
未验证窗格需要 `--force`。

## 运行时后端

### tmux

配置 `backend = "tmux"` 时，Workhive：

- 创建或复用共享 tmux server，并创建唯一命名的 session；
- 根据布局创建 pane 并启动命令；
- 提供 tmux attach、detach、pane split/close/send、pane 标题、鼠标模式和 OSC 52
  剪贴板配置；
- 观测窗格前台进程和终端状态，并可提供活动状态与本地通知。

默认 session 名称：

```text
wh@{instance_id}@{project}@{workspace}
```

默认共享 tmux server 名称为 `default`。可以从另一个 tmux client 执行
`workspace open`、`workspace attach` 和 `session attach`。

### Herdr

配置 `backend = "herdr"` 时，Workhive：

- 按 Workhive 的 Git/worktree 策略创建或采用 checkout；
- 使用 `worktree.open` 将已有 checkout 注册到 Herdr；
- 通过 Herdr 应用配置的布局；
- 列出、聚焦、切分、关闭、重命名 Herdr pane，并向其发送输入；
- 通过 `session list` 展示 Herdr 原生的 session → workspace → tab → pane 层级，
  并把整个 session 的接入交给 Herdr。

Workhive 使用 Herdr 的 workspace identity 和持久化 binding。Herdr 需要兼容的运行中
server；配置为 Workhive 保留的 named session 时，Workhive 可以通过 Herdr 文档化的
detached launcher 启动缺失的 server。自定义 session binding 会继续关联其持久化的
endpoint。

全局 Herdr session 默认值为 `workhive-managed`，项目配置中的 `herdr.session` 可以覆盖它。
已有 workspace binding 会保持权威，只有对选定工作区显式使用 `--force` 重建后才应用新的
session 配置。

## 配置

配置使用 TOML，后端值为 `tmux` 或 `herdr`。项目配置覆盖全局配置。

```bash
workhive init --global --backend tmux
workhive config set backend herdr --global
workhive config set herdr.session my-session
workhive config set tmux.mouse false --global
workhive config set tmux.clipboard false
```

其中 `tmux.mouse` 和 `tmux.clipboard` 只影响 tmux 后端。

重要路径：

| 路径 | 用途 |
|------|------|
| `~/.config/workhive/config.toml` | 全局配置 |
| `~/.config/workhive/.env` | 全局环境变量 |
| `~/.config/workhive/projects.json` | 项目索引 |
| `<project>/.workhive/config.toml` | 项目配置 |
| `<project>/.workhive/.env` | 项目环境变量 |
| `<project>/.workhive/instance_id` | 项目稳定标识，用于 tmux 命名 |
| `<project>/.workhive/worktrees/` | Workhive 管理的 Git worktree |
| `<project>/.workhive/runtime/` | 运行时控制和 pane identity 状态 |

`workspace open` 会把生效的 `.env` 变量注入它启动的程序。语法是 `KEY=VALUE`；无效
行会产生警告，注入摘要不会打印原始 value。

初始化向导可以配置布局和 pane 启动命令。内置预设包括 `claude`、`codex`、`opencode`、
`lazygit`、`yazi`、`htop` 和 `btm`，前提是这些命令存在于 `$PATH`。

可选配置组包括：

- `[features.ai_integrations]`：provider 活动信号和可选的 Codex `notify` 包装器；
- `[features.pane_observation.terminal_status]`：终端状态观测；
- `[features.notifications]`：本地通知。

这些功能用于显示窗格活动状态和发送本地通知。

## Shell 补全

```bash
workhive completion install
workhive completion install bash
workhive completion install zsh
workhive completion install fish
workhive completion script bash
```

支持的脚本类型为 `bash`、`zsh`、`fish`、`powershell` 和 `elvish`。

## 安全和非交互使用

创建项目状态前，项目根目录必须位于可信目录下。脚本或 CI 可以先添加父目录：

```bash
workhive project roots add /path/to/parent
workhive init
workhive workspace open feature-auth --no-attach --init
```

交互选择器和确认提示需要 TTY。自动化调用应显式传入工作区名称，并按需使用
`--force`/`-f`。机器可读输出使用 `--view json`。

## 系统要求

| 工具 | 需要时 | 用途 |
|------|--------|------|
| Git | 始终需要 | 仓库和 worktree 管理 |
| tmux | `backend = "tmux"` | session 和 pane |
| Herdr server | `backend = "herdr"` | Herdr workspace、tab 和 pane |
| Rust stable 1.95.0+ | 从源码构建 | 构建工具链 |

## 许可证

MIT
