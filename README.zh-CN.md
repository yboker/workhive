[English](README.md) | [中文](README.zh-CN.md)

# workhive

[License: MIT](LICENSE)
[Shell: Bash 4.3+](https://www.gnu.org/software/bash/)
[Platform: macOS / Linux / WSL2]()

**一条命令，编排你的窗格、命令与会话。**  
基于 Git Worktree 与 tmux 的终端工作区编排器。

[痛点](#你是否遇到过这些问题) • [方案](#workhive-如何解决) • [快速开始](#快速开始) • [命令手册](#命令手册) • [最佳实践](#最佳实践) • [配置](#配置文件)

扩展文档索引见 [`help/README.md`](help/README.md)。

---

## 你是否遇到过这些问题？

### 分支切换地狱

> 正在 `feature-a` 上跑 Claude Code，突然要 hotfix。`git stash` → 切分支 → 修 bug → 切回来 → `git stash pop` → 冲突了...

workhive 让每个分支拥有**完全隔离的工作目录**，并行开发互不干扰。切分支？不存在的——你同时打开多个分支，各自独立。

### SSH 断线，环境全没了

> 远程 SSH 到服务器跑 Claude Code，网络一抖，连接断了。进程没了，上下文没了，得从头来。

workhive 基于 tmux 管理所有会话。**只要 tmux 会话仍在运行，断线就不会丢失环境**。重新 SSH 进来，`workhive attach feature-x`，三分屏布局、Claude Code 对话、Lazygit 状态——一切都在，原封不动。

### 每次开工都在搭环境

> 打开终端，手动分屏，启动 Claude Code，再开一个 pane 跑 Lazygit，再开一个 Shell 看 diff... 每天重复这套操作。

`workhive open feature-x`，一条命令搞定。自动创建 worktree、启动 tmux 会话、三分屏布局、在每个 pane 里启动你配置好的工具。**零手动操作，即刻进入心流。**

### 人在外地，服务器上的环境够不着

> 出差在外，手机收到告警。SSH 跳板机连进去，但之前的 tmux 会话？不记得叫什么了。服务跑在前台，早就随着断连一起没了。

workhive 基于 tmux 会话管理和快照恢复。所有进程跑在 tmux 里，断连通常不影响；`workhive ls` 看看有哪些活跃会话，`workhive attach` 可一键重连到运行中的会话，必要时还能用快照重建布局与命令。

### AI Coding 时窗口不够用

> 一边 Claude Code 写代码，一边想看 git diff review 变更，还想留个 Shell 跑测试。手动分屏、调大小、切来切去，注意力全碎了。

workhive 的三分屏布局就是为 AI Coding 设计的：Claude Code 占一块，Lazygit 占一块，Shell 留一块。**布局自动配好，专注写代码就行。**

---

## workhive 如何解决


| 痛点              | workhive 的方案                                      |
| --------------- | ------------------------------------------------- |
| 切分支要 stash / 冲突 | 每个分支独立 Git Worktree，并行开发零干扰                       |
| SSH 断线丢环境       | tmux 会话保活 + 快照恢复，`workhive attach` 一键重连运行中的会话            |
| 每天手动搭环境         | `workhive open` 一条命令：worktree + tmux + 三分屏 + 工具启动 |
| 远程救火找不到会话       | `workhive ls` 列出所有空间，`workhive attach` 直连       |
| AI Coding 窗口管理烦 | 预设三分屏布局，Claude Code + Lazygit + Shell 各就各位        |
| 环境配置不可复现        | `.workhive/config.yaml` 版本化，团队共享同一套环境             |


### 谁适合用 workhive？

- **AI Coding 重度用户** — 日常使用 Claude Code / Codex / OpenCode 等 AI 工具，需要多窗口协作
- **远程开发者** — SSH 到服务器开发，需要断线恢复与快照恢复能力
- **多分支并行开发者** — 同时推进多个 feature / hotfix，受够了 stash 和分支切换
- **CLI 极客** — 偏好终端工作流，追求效率和自动化

### 它不是什么

workhive **不是**终端模拟器，它是**终端内的工作流编排器**。你用什么终端都行，workhive 管理的是终端里面的开发环境。

---

## 核心特性

- **确定性的 `open` 行为** — `workhive open <branch>` 固定按“复用现有会话 -> 恢复健康快照 -> 从 `.workhive/config.yaml` 重建”执行
- **可强制重建** — `workhive open -f <branch>` 跳过复用/恢复，严格按配置重建
- **独立 tmux runtime** — 每个项目实例使用自己的 tmux server，不污染默认 tmux server
- **并行分支隔离** — 每个分支拥有独立 worktree、session、pane 和 runtime metadata
- **轻重分离的状态采集** — `workhive ls` 走批量采集；`--watch` 与 `--format json` 走更轻的 fast 策略
- **内置快照恢复** — 保存/恢复 tmux 的窗口、pane、布局、工作目录和配置命令
- **统一 pane 交互语义** — 鼠标滚动按 pane 语义（`agent` / `tool` / `shell`）处理，不按具体命令名打补丁
- **跨平台** — 支持 macOS / Linux / WSL2

### 布局示意图 (Column-based DSL)

```
AI Coding (2列, 2+1 面板):        Quad (2列, 2+2 面板):
┌──────────┬──────────────┐      ┌──────────┬──────────┐
│ Claude   │              │      │ Claude   │ Lazygit  │
│ (60%,50%)│  Lazygit     │      │ (50%,50%)│ (50%,70%)│
├──────────┤  (40%,100%)  │      ├──────────┼──────────┤
│ Shell    │              │      │ Shell    │ yazi     │
│ (60%,50%)│              │      │ (50%,50%)│ (50%,30%)│
└──────────┴──────────────┘      └──────────┴──────────┘

3列 (3列, 2+3+1 面板):           单列 (1列, 3 面板):
┌────────┬────────┬────────┐     ┌─────────────────────┐
│ C1P1   │ C2P1   │        │     │ Pane 1 (100%,33%)   │
│ (33%)  │ (33%)  │        │     ├─────────────────────┤
├────────┼────────┤ C3P1   │     │ Pane 2 (100%,33%)   │
│ C1P2   │ (33%)  │ (34%)  │     ├─────────────────────┤
│ (33%)  ├────────┤        │     │ Pane 3 (100%,34%)   │
│        │ C2P3   │        │     └─────────────────────┘
└────────┴────────┴────────┘
```

---

## 系统要求

### 必需依赖


| 依赖          | 最低版本  | 用途              | 安装方法                                                                                               |
| ----------- | ----- | --------------- | -------------------------------------------------------------------------------------------------- |
| Git         | 2.20+ | 版本控制 & Worktree | macOS: `brew install git`，Linux: `apt install git`                                                 |
| tmux        | 3.2+  | 终端多路复用          | macOS: `brew install tmux`，Linux: `apt install tmux`                                               |
| yq          | 4.x   | YAML 配置解析       | macOS: `brew install yq`，Linux: [安装指南](https://github.com/mikefarah/yq#install)                    |

> **注意:** Bash 4.3+ 是必需的 Shell 运行环境，但大多数系统已预装。只有 Git、tmux 和 yq 需要手动安装。

### Pane 工具 (可选，可在 pane 中启动)

| 工具                                               | 用途                    | 安装方法                                                                                  |
| ------------------------------------------------ | --------------------- | ------------------------------------------------------------------------------------- |
| [Claude Code](https://github.com/anthropics/claude-code) | AI 编程助手        | `npm install -g @anthropic-ai/claude-code`                                                        |
| [Codex](https://github.com/openai/codex)         | OpenAI AI 编程代理        | `npm install -g @openai/codex`                                                        |
| [OpenCode](https://github.com/anomalyco/opencode) | AI 编程代理               | `npm i -g opencode-ai@latest`                                  |
| [Lazygit](https://github.com/jesseduffield/lazygit) | Git TUI 界面      | macOS: `brew install lazygit`，Linux: [安装指南](https://github.com/jesseduffield/lazygit#installation) |
| [yazi](https://github.com/sxyazi/yazi)           | 终端文件浏览器 | macOS: `brew install yazi`，Linux: [安装指南](https://github.com/sxyazi/yazi#installation) |

> **Pane 工具回退:** 在 `workhive init` 期间，如果模板里引用的 pane 工具未安装，该 pane 会回退为空 shell。之后可通过 `workhive config` 调整 pane 命令。

**支持平台**: macOS / Linux / WSL2

### Runtime 行为

workhive 内置了 tmux 快照恢复模块，可以保存和恢复窗口、pane、布局、工作目录和配置命令，但**不会恢复终端滚动历史**。配置项见 `tmux.persistence.*`。

鼠标滚动也由 runtime 策略统一控制：

- `workspace` 模式：agent pane 可通过 tmux copy-mode 滚动历史，shell pane 不会被隐式切进 copy-mode
- `compatible` 模式：更接近 tmux 默认滚轮行为

---

## 快速开始

### 1. 安装

**方式 A：脚本安装（推荐）**

```bash
# 安装最新版本
curl -fsSL https://raw.githubusercontent.com/yboker/workhive/main/install.sh | bash

# 安装指定版本
curl -fsSL https://raw.githubusercontent.com/yboker/workhive/main/install.sh | bash -s -- --version v0.8.0
```

**方式 B：克隆仓库**

```bash
# 克隆仓库
git clone https://github.com/yboker/workhive.git

# 添加到 PATH (推荐)
echo 'export PATH="$PATH:/path/to/workhive/bin"' >> ~/.zshrc
source ~/.zshrc

# 或创建别名 (可选快捷方式)
echo 'alias wh="/path/to/workhive/bin/workhive"' >> ~/.zshrc
source ~/.zshrc
```

请使用安装脚本或直接克隆仓库。

### Shell 补全

Tab 补全支持 **Zsh**、**Bash** 和 **Fish**。

**推荐一键配置** — 由 `workhive shell` 统一托管：

```bash
workhive shell setup --profile minimal --enable completions --yes
```

### 2. 初始化项目

在你的项目根目录中运行：

```bash
cd your-project
workhive init
```

交互式向导会引导你配置：

- 列式布局 (Column-based DSL) 和各 Pane 的启动命令
- tmux 主题选择（实时预览）

生成 `.workhive/config.yaml` 项目配置文件，用于工作区编排。

> **写入安全与一致性：** `workhive init`（项目级/全局）采用事务化 staging + 原子替换。  
> `workhive config` 与 `workhive tools` 也采用原子写入，提升可靠性并降低中断导致的配置损坏风险。

### 3. 开始开发

```bash
# 创建并进入 Worktree
workhive open feature-x

# 在另一个终端同时开发另一个分支
workhive open bugfix-123

# 查看所有空间
workhive ls

# SSH 断线后恢复
workhive attach feature-x

# 完成后销毁空间
workhive rm feature-x
```

**`open` 行为说明：**

- `workhive open <branch>`：固定策略为“优先复用已有 tmux 会话，其次尝试恢复健康快照，最后按 `.workhive/config.yaml` 重建布局与命令”。
- `workhive open -f <branch>`：强制按 `.workhive/config.yaml` 重建。
- 在目标 session 内执行 `workhive open -f <branch>`：依然会强制按 `.workhive/config.yaml` 重建；workhive 会先分离当前 client，再原地重建目标 session。

**`ls` 行为说明：**

- `workhive ls` 交互表格使用 full 策略，会收集 dirty status。
- `workhive ls --watch` 使用 fast 策略，避免反复重型 Git 扫描。
- `workhive ls --format json` 也使用 fast 策略，保证机器读取输出更稳定。

**独立 tmux runtime：**

workhive 为每个项目实例创建独立 tmux server，命名格式为 `workhive-<project>-<instance_id>`。所有 session、pane、hook、快照恢复都只作用于这个独立 server，不影响你的默认 tmux server。`workhive attach` 会自动选择合适的 attach 策略：

- **同一 server 内**：使用 `switch-client`
- **跨 server**：启动新的 `tmux attach`
- **tmux 外部**：直接 `tmux attach`

**鼠标滚动语义：**

- Claude / Codex / Hermes 这类 agent pane 的滚动依赖 pane metadata（`@workhive_pane_kind=agent`），不是按命令名特判。
- shell pane 保持更安全的 workspace 行为；如果你要更接近 tmux 默认滚轮，可切换 `tmux.interaction_profile=compatible`。

---

## 命令手册

### 命令总览


| 命令                 | 缩写            | 说明                       |
| ------------------ | ------------- | ------------------------ |
| `workhive init`    | —             | 交互式初始化配置                 |
| `workhive config`  | —             | 查看/修改配置                  |
| `workhive tools`   | —             | 管理全局工具注册表                |
| `workhive open`    | —             | 创建/进入 Worktree           |
| `workhive attach`  | —             | 附加到现有会话                  |
| `workhive close`   | —             | 分离或终止当前工作空间              |
| `workhive pane`    | —             | 管理 pane (开启 / 关闭 / 分割) |
| `workhive list`    | `workhive ls` | 全局工作空间仪表盘（所有空间）      |
| `workhive remove`  | `workhive rm` | 彻底销毁 Worktree            |
| `workhive doctor`  | —             | 环境健康检查                   |
| `workhive rename`  | —             | 重命名工作空间 (worktree + 会话 + 状态) |
| `workhive gc`      | —             | 垃圾回收: 清理已合并分支和过期资源       |
| `workhive destroy` | —             | 彻底清除所有 workhive 资源       |
| `workhive shell`   | —             | 管理 Shell 集成（别名、提示符）     |
| `workhive upgrade` | —             | 检查更新并升级                  |
| `workhive log`     | —             | 查看 workhive 日志            |


### `workhive init` — 初始化配置

在当前 Git 仓库中创建 `.workhive/config.yaml` 配置文件，或通过 `--global` 创建全局配置。

```bash
workhive init                      # 交互式配置向导 (项目级)
workhive init --global             # 初始化全局配置 (用户级默认)
workhive init --non-interactive    # 使用默认配置，跳过交互
workhive init --force              # 强制覆盖现有配置
```

**项目模式** (`workhive init`): 仅检查项目级配置。若 `.workhive/config.yaml` 不存在，进入完整配置向导（选语言 → 配置列式布局 → 选主题）。同时生成唯一 instance ID 用于会话隔离。

**全局模式** (`workhive init --global`): 仅检查全局配置。创建适用于所有项目的用户级默认配置。


| 选项                  | 说明             |
| ------------------- | -------------- |
| `--global`          | 初始化全局配置 (`~/.config/workhive/config.yaml`) |
| `--non-interactive` | 使用默认配置，跳过所有交互  |
| `--force`           | 强制重新初始化，覆盖现有配置 |
| `-h, --help`        | 显示帮助信息         |


**执行流程:**

1. 检测系统依赖 (Git, tmux, yq)
2. 配置三分屏布局和各 Pane 命令
3. 选择 tmux 主题
4. 生成 `.workhive/config.yaml`

---

### `workhive config` — 配置管理

查看和修改现有的 workhive 配置。

```bash
workhive config                    # 交互式查看/修改配置
workhive config --edit             # 直接使用编辑器打开配置文件
workhive config set <key> <value>  # 设置配置值
workhive config set <key> <value> --global  # 设置全局配置值
workhive config get <key>          # 获取配置值
workhive config get <key> --global # 获取全局配置值
workhive config git-whitelist add <path>    # 添加白名单目录
workhive config git-whitelist remove <path> # 移除白名单目录
workhive config git-whitelist list          # 列出白名单
```


| 子命令 / 选项 | 说明 |
|--------------|------|
| `set <key> <value>` | 设置配置值 (点号分隔的键路径，如 `tmux.theme`) |
| `get <key>` | 获取配置值 |
| `git-whitelist add <path>` | 将目录添加到 Git 初始化白名单（全局配置） |
| `git-whitelist remove <path>` | 从 Git 初始化白名单移除目录 |
| `git-whitelist list` | 列出当前白名单中的所有目录 |
| `--edit` | 直接使用编辑器打开配置文件 |
| `--global` | 操作全局配置而非项目配置 |
| `-h, --help` | 显示帮助信息 |

**示例:**

```bash
workhive config set tmux.theme tokyo-night
workhive config set tmux.persistence.enabled true --global
workhive config get tmux.theme
```

---

### `workhive tools` — 全局工具注册表

管理全局工具注册表（`~/.config/workhive/config.yaml`）。

```bash
workhive init --global
workhive tools list
workhive tools add --name claude --cmd "claude --dangerously-skip-permissions" --ai true --default true
workhive tools edit claude --cmd "claude --resume"
workhive tools remove claude
workhive tools set-default codex
workhive tools validate
workhive tools manage
```

| 子命令 | 说明 |
|--------|------|
| `list` | 列出已配置工具 |
| `add` | 添加工具（必须提供 `--name` 和 `--cmd`） |
| `edit` | 编辑现有工具 |
| `remove` | 删除工具 |
| `set-default` | 设置默认工具 |
| `validate` | 校验工具配置 |
| `manage` | 交互式管理工具 |
| `-h, --help` | 显示帮助 |

> `tools` 仅支持全局配置，项目级 tools 配置会被忽略。首次使用前请先执行 `workhive init --global`。

---

### `workhive open [worktree]` — 创建/进入 Worktree

为指定分支创建/进入完整隔离开发环境，行为固定为三段式：

1. 优先复用已存在 tmux 会话（直接进入）
2. 否则尝试从健康快照恢复会话
3. 若快照不可用或恢复失败，按 `config.yaml` 重建布局与命令

```bash
workhive open feature-x                # 创建 Worktree + 会话 + 布局
workhive open                          # 交互式选择: 已有空间 / 新分支 / 主目录
workhive open feature-x --force        # 强制销毁旧会话，按 config.yaml 完全重建
workhive open feature-x --no-attach    # 创建后不自动进入 (后台运行)
```


| 参数/选项            | 说明                                |
| ---------------- | --------------------------------- |
| `<worktree>`     | Worktree 名称（即 Git 分支名），省略则弹出交互式菜单 |
| `--force, -f`    | 强制按 `config.yaml` 重建（销毁旧会话）       |
| `--no-attach`    | 创建后不自动进入（适合批量创建）                  |
| `-h, --help`     | 显示帮助信息                            |


**执行流程:**

1. 创建 Git Worktree → `.workhive/worktrees/<name>`
2. 创建 tmux 会话 → `<project>-<name>` (project = 项目目录名)
3. 分割为三分屏布局，启动配置中的命令
4. 附加到会话

> **open 与 attach 的区别:**
>
> - `workhive open` — 按固定策略进入：先复用会话，再尝试快照恢复，最后才按 `config.yaml` 重建。
> - `workhive attach` — 仅附加到已存在的 tmux 会话，不会创建 Worktree 或会话。
>
> **何时使用哪个命令:**
>
>
> | 场景                      | 命令                                                                                    |
> | ----------------------- | ------------------------------------------------------------------------------------- |
> | 首次进入某个分支                | `workhive open feature-x`                                                             |
> | SSH 重连 / detach 恢复（会话仍在运行） | `workhive attach feature-x`                                                           |
> | 修改 `config.yaml` 后强制按模板重建 | `workhive open feature-x -f`                                                          |
> | 完全重置（销毁并重建）             | `workhive open feature-x --force`                                                     |
>

---

### `workhive attach [session|branch]` — 附加到现有会话

重新连接到已存在的 tmux 会话。

```bash
workhive attach feature-x                # 通过分支快捷名附加到会话
workhive attach wh@id@proj@feature-x    # 通过完整会话名附加
workhive attach                          # 交互式选择: 从活跃会话中选择
workhive attach main -r                  # 只读模式 (tmux 3.0+)
workhive attach feature-x --force-layout # 按最新 config.yaml 重建布局
```


| 参数/选项             | 说明                              |
| ----------------- | ------------------------------- |
| `<session|branch>` | 已存在的会话名，或对应分支快捷名；省略则弹出交互式菜单      |
| `--force-layout`  | 强制按 `config.yaml` 重建布局（修改配置后使用） |
| `-r, --read-only` | 只读模式，仅查看不可操作 (需 tmux 3.0+)      |
| `-h, --help`      | 显示帮助信息                          |


**智能行为:**

- 如果已在 tmux 中，自动使用 `switch-client` 切换，避免嵌套
- 如果不在 tmux 中，直接 `attach`
- 如果目标会话不存在，attach 会失败并列出活跃会话；创建新空间请用 `workhive open <worktree>`

---

### `workhive close` — 关闭当前工作空间

> **注意:** 此命令只能在 tmux pane 内使用 (即在 workhive 工作空间内部)。

关闭或分离当前 workhive 工作空间。提供 TUI 选择菜单，有两个选项:

1. **仅分离** (默认，推荐) — 会话在后台继续运行。稍后可通过 `workhive attach` 重新进入。
2. **终止会话** — 完全终止 tmux 会话。

```bash
workhive close                  # 交互式: 选择分离或终止
workhive close --force          # 立即分离 (保留会话)
workhive close --kill           # 直接终止会话 (跳过选择菜单)
```

| 选项 | 说明 |
|------|------|
| `--force, -f` | 立即分离 (跳过交互菜单，保留会话) |
| `--kill` | 直接终止会话 (跳过选择菜单) |
| `-h, --help` | 显示帮助 |

**注意:**
- **分离** 保留会话状态 — 所有窗格、运行中的进程和布局都保持不变
- **终止** 仅关闭 tmux 会话，**不会**删除 worktree 或分支
- 彻底删除工作空间请使用: `workhive rm <branch>`
- 分离后重新进入: `workhive attach <branch>`

> **close 与 rm 的区别:**
>
> | 命令 | 会话 | Worktree | 分支 |
> |------|------|----------|------|
> | `workhive close` (分离) | ⏸️ 保留 | ❌ 保留 | ❌ 保留 |
> | `workhive close` (终止) | ✅ 关闭 | ❌ 保留 | ❌ 保留 |
> | `workhive rm <branch>` | ✅ 关闭 | ✅ 删除 | ✅ 删除 |

---

### `workhive list` / `workhive ls` — 全局工作空间仪表盘

显示所有 Worktree 工作空间的综合状态仪表盘，包含会话状态、布局有效性、Git 状态和最后活动时间。

```bash
workhive list                    # 表格格式仪表盘
workhive ls --format json        # JSON 格式 (脚本使用)
workhive list --layout           # 显示布局配置详情
workhive list --watch            # 监视模式 (定期刷新)
workhive list --interval 2       # 监视模式刷新间隔 (秒)
```


| 选项                                    | 说明                  |
| ------------------------------------- | ------------------- |
| `--format <table\|json>`               | 输出格式，默认: table      |
| `--layout`                            | 显示布局配置详情           |
| `--watch`                             | 监视模式，定期刷新         |
| `--interval <seconds>`                | 监视模式刷新间隔 (默认 5)  |
| `-h, --help`                          | 显示帮助信息              |


**输出列说明:**


| 列名          | 说明                                     |
| ----------- | -------------------------------------- |
| 分支          | Git 分支名                                |
| Worktree    | Worktree 目录是否存在 (✓/✗)                  |
| 会话          | 会话状态: ● active / ○ detached / - none   |
| 布局          | 布局是否有效 (✓/✗)                           |
| Git 状态      | 工作区状态: clean / 3M 1? / -               |
| 最后活动        | 距上次活动时间: just now / 5m ago / 2h ago    |


**使用场景:**

- **一览全局** — 在一个视图中查看所有工作空间的健康状态和 Git 状态
- **脚本** — 使用 `--format json` 以编程方式获取工作空间状态
- **发现闲置空间** — 识别长时间无活动的工作空间

> 注: 当前版本仅保留 `list/ls` 作为工作空间总览命令。

---

### `workhive pane` — 管理 Pane (开启 / 关闭 / 分割 / 发送 / 列表)

管理当前 tmux 会话中的 pane。七个子命令:

```bash
# 在当前 pane 中打开工具
workhive pane open                        # 交互式: 选择 Worktree → 选择命令
workhive pane open dev01                  # cd 到 dev01 后选择命令
workhive pane open dev01 --cmd claude     # 非交互式: 直接打开 claude
workhive pane open dev01 --cmd claude -f  # 自动创建 worktree + 打开 claude

# 关闭当前 pane
workhive pane close                  # 关闭 pane (需确认)
workhive pane close -f               # 关闭 pane (跳过确认)

# 在当前 pane 下方分割
workhive pane split                  # 下方新建 shell
workhive pane split --cmd lazygit    # 下方新建 pane 并运行 lazygit

# 向指定 pane 发送文本
workhive pane send 1 "npm test"      # 向 pane 1 发送 "npm test" + 回车
workhive pane send 2 "hello" --commit none  # 发送文本不按回车
workhive pane send 3 "执行任务" --mode auto --commit auto  # 自动适配注入/提交策略
workhive pane send 1,2,3 "npm test"  # 同时发送到指定 pane
workhive pane send --all "npm test"  # 向当前会话全部 pane 广播

# 列出 pane
workhive pane list                   # 列出 pane 运行态 + AI Agent 状态

# 查看当前 pane 详情
workhive pane current

# 修改布局配置
workhive config --layout
workhive config --global --layout
```

| 子命令/选项 | 说明 |
|---|---|
| `open [worktree]` | 在当前 pane 中打开工具 |
| `open --cmd <alias>` | 跳过交互式菜单; 别名: `claude`, `codex`, `opencode`, `lazygit`, `yazi`, `shell` |
| `open -f, --force` | Worktree 不存在时自动创建 (跳过确认) |
| `close` | 关闭当前 pane (不能关闭最后一个 pane) |
| `close -f, --force` | 跳过确认关闭 (AI 自动化专用) |
| `split [--cmd <cmd>]` | 在当前 pane 下方分割, 保持左右布局 |
| `split --size <pct>` | 自定义分割比例 (默认: 50) |
| `send <pane> <text>` | 向指定 pane 发送文本 (literal 模式, 支持中文) |
| `send <p1,p2,...> <text>` | 向多个指定 pane 同时发送文本 |
| `send --all <text>` | 向当前会话全部 pane 广播文本 |
| `send --commit none` | 发送文本但不按回车 |
| `list` | 列出所有 pane 的索引、命令、目录、大小、状态、AI Agent 和 Agent 状态 (`running`/`waiting`/`idle`/`unknown`) |
| `current` | 查看当前 pane 的详细信息 (支持 `--format table|json`) |
| `layout` | 运行时重新配置列式布局 (交互式) |
| `-h, --help` | 显示帮助 (每个子命令有独立的 --help) |

**设计:** 每个子命令有独立的 `--help` 输出。例如 `workhive pane split --help` 显示分割专用帮助, `workhive pane --help` 显示总览。

> **命令层级:**
>
> - `workhive open` — **空间级**: 创建或进入完整的 Worktree 工作空间 (含 tmux 会话 + 布局)
> - `workhive pane open` — **Pane 级**: 在当前 pane 中打开工具

### 多任务并行开发 (AI Skill)

workhive 内置 AI 编码助手技能 (Claude Code, Codex, OpenCode), 支持**多任务并行开发**。当你提供编号任务列表时, AI 可以:

1. **分析与分组** 相关任务为模块
2. **创建 worktree** 并在并行 pane 中打开 AI 工具
3. **分发任务** 到每个 pane 的 AI 实例
4. **监控进度** 通过 git log 和 pane 输出
5. **合并与清理** 所有分支回主分支

**触发短语:** "使用 workhive 多任务并发完成"、"多 agent 同时进行"、"拆分任务到多个分支"、"并行开发", 或直接提供编号任务列表。

**示例工作流:**
```bash
# AI 分割 pane 并为每个任务组打开 claude
workhive pane split --cmd "workhive pane open task-auth --cmd claude -f"
workhive pane split --cmd "workhive pane open task-search --cmd claude -f"

# AI 向每个 pane 发送任务
workhive pane send 2 "实现用户登录和注册功能。完成后请 commit。"
workhive pane send 3 "创建搜索 API 和结果页面。完成后请 commit。"

# AI 监控并在完成后合并
git merge task-auth --no-ff -m "merge: auth features"
workhive rm task-auth --force
```

技能文件位于 `skills/workhive/SKILL.md`, Claude Code、Cursor 等 AI 工具会自动加载

---

### `workhive list` / `workhive ls` — 列出当前项目的所有空间

查看当前项目的所有空间状态（包含主分支基线与已管理 Worktree，仅显示属于当前项目的工作空间）。

```bash
workhive ls                        # 表格格式 (默认)
workhive ls --format json          # JSON 格式
workhive list --layout             # 显示布局配置详情
workhive list --watch              # 监视模式 (定期刷新)
workhive list --interval 2         # 监视模式刷新间隔 (秒)
```


| 选项                       | 说明                  |
| ------------------------ | ------------------- |
| `--format <table\|json>` | 输出格式，默认: table      |
| `--layout`               | 显示布局配置详情           |
| `--watch`                | 监视模式，定期刷新         |
| `--interval <seconds>`   | 监视模式刷新间隔 (默认 5)  |
| `-h, --help`             | 显示帮助信息              |


**表格输出示例:**

```
分支                 Worktree   会话         布局       Git 状态       最后活动
────────────────────────────────────────────────────────────────────────────────
main                 ✗          - none       ✗          -              -
feature-x            ✓          ● active     ✓          clean          2m ago
```

---

### `workhive remove` / `workhive rm` — 销毁 Worktree

关闭 tmux 会话，可选删除 Worktree 和 Git 分支。

```bash
workhive rm feature-x                    # 彻底销毁 (会话 + Worktree + 分支)
workhive rm feature-x --keep-worktree    # 仅关闭会话，保留代码和分支
workhive rm feature-x --force            # 强制删除 (丢弃未提交更改)
```


| 参数/选项               | 说明                         |
| ------------------- | -------------------------- |
| `<worktree>`        | 空间名称（必需）                   |
| `--keep-worktree`   | 保留 Worktree 代码和 Git 分支     |
| `--remove-worktree` | 删除 Worktree 及 Git 分支（默认行为） |
| `--force`           | 强制删除，跳过未提交更改确认             |
| `-h, --help`        | 显示帮助信息                     |


**执行流程:**

1. 关闭 tmux 会话
2. 删除 Worktree 目录（可选）
3. 删除 Git 分支（可选）

默认行为由 `config.yaml` 中 `worktree.auto_delete` 控制。有未提交更改时会提示确认。

---

### `workhive doctor` — 环境健康检查

运行全面的环境健康检查。验证依赖项、配置文件、tmux 兼容性和 Git 仓库状态。

```bash
workhive doctor                  # 表格格式健康报告
workhive doctor --format json    # JSON 格式 (CI/脚本使用)
```


| 选项                        | 说明                  |
| ------------------------- | ------------------- |
| `--format <table\|json>`   | 输出格式，默认: table      |
| `-h, --help`              | 显示帮助信息              |


**检查项:**


| 分类       | 工具                                 | 严重程度                |
| -------- | ---------------------------------- | ------------------- |
| 必需依赖     | git, tmux, yq                      | ✗ 错误 (阻止使用)         |
| Pane 工具   | lazygit, claude, codex, opencode, yazi | ○ 警告 (功能受限)         |
| 配置验证     | `.workhive/config.yaml` 语法         | ✗ 错误 (如果无效)         |
| tmux 版本  | Popup 支持 (3.3+)                    | ○ 警告 (低于 3.3)       |
| Git 仓库   | 工作区检测                              | ○ 信息                |


**使用场景:**

- **初始安装** — 首次使用前验证所有依赖是否已安装
- **故障排查** — 诊断功能异常原因 (如缺少 fzf 导致模糊选择不可用)
- **CI/CD** — 使用 `--format json` 在自动化流水线中验证环境就绪状态
- **团队入职** — 分享 `workhive doctor` 输出以识别缺失的工具

---

### `workhive gc` — 垃圾回收

清理已合并分支的 Worktree、无对应 Worktree 的孤立会话、以及过期的状态文件。保持工作空间整洁。

```bash
workhive gc                # 交互式清理 (需确认)
workhive gc --auto         # 自动清理，不需确认
workhive gc --snapshots    # 仅清理快照文件
```


| 选项           | 说明            |
| ------------ | ------------- |
| `--auto`     | 自动清理，不需确认     |
| `--snapshots`| 仅清理快照文件 (`.workhive/snapshots/`) |
| `-h, --help` | 显示帮助信息        |


**清理内容:**


| 资源类型          | 清理条件                                | 执行操作                    |
| ------------- | ----------------------------------- | ----------------------- |
| 已合并分支 Worktree | 分支已合并到主分支                           | 删除 Worktree + 会话 + 分支   |
| 孤立会话          | tmux 会话存在但无对应 Worktree               | 终止会话                    |
| 过期状态文件        | `.workhive/state/` 中无对应 Worktree 或会话 | 删除文件                    |


**使用场景:**

- **PR 合并后清理** — 合并 PR 后，一条命令清理残留的 Worktree 和会话
- **磁盘空间回收** — 删除不再需要的分支对应的 Worktree
- **会话卫生** — 终止失去 Worktree 的孤立 tmux 会话
- **自动化维护** — 在 cron 任务或 CI 中使用 `--auto` 定期清理
- **快照重置** — 使用 `--snapshots` 仅清理会话快照，不影响 Worktree/分支

**执行流程:**

1. 扫描已合并到主分支的分支 → 删除其 Worktree、会话和分支
2. 检测孤立会话 (有会话但无 Worktree) → 终止会话
3. 查找过期状态文件 → 删除文件
4. 执行 `git worktree prune` 清理 Git 的 Worktree 记录

---

### `workhive destroy` — 彻底清除所有 workhive 资源

关闭所有会话、删除所有 Worktree、删除 `.workhive/` 目录，恢复到未初始化状态。**不可逆操作。**

```bash
workhive destroy                  # 交互式确认后彻底清除
workhive destroy --force          # 跳过确认，直接清除
```


| 选项           | 说明       |
| ------------ | -------- |
| `--force`    | 跳过所有确认提示 |
| `-h, --help` | 显示帮助信息   |


**此命令会:**

1. 关闭所有 workhive 管理的 tmux 会话
2. 删除所有 Worktree（代码 + Git 分支）
3. 删除 `.workhive/` 配置目录（配置、日志、缓存）

**注意:** 此操作不可逆！所有未提交的代码将丢失。不会删除项目代码本身和 `.git` 目录。执行后需重新 `workhive init` 初始化。

---

### `workhive rename` — 重命名工作空间

重命名 workhive 工作空间 — Git 分支、Worktree 目录、tmux 会话和状态文件全部原子更新，失败时自动回滚。

```bash
workhive rename old-branch new-branch         # 重命名工作空间
workhive rename old-branch new-branch --force # 即使会话已附加也强制重命名
```

| 参数/选项 | 说明 |
|----------|------|
| `<old_branch>` | 当前分支/worktree 名称 |
| `<new_branch>` | 新分支/worktree 名称 |
| `--force, -f` | 即使会话已附加也强制重命名 |
| `-h, --help` | 显示帮助 |

**执行流程:** 重命名 Git 分支 → 移动 Worktree 目录 → 重命名 tmux 会话 → 重命名状态文件。旧分支不存在或新名称已被占用时失败。不能重命名主分支。使用事务模式，任何步骤失败都会自动回滚。

---

### `workhive log` — 查看日志

查看 workhive 操作日志，支持过滤和实时跟踪。

```bash
workhive log                        # 显示最后 50 行
workhive log --follow               # 实时跟踪 (类似 tail -f)
workhive log --lines 100            # 显示最后 100 行
workhive log --level error          # 按级别过滤: debug|info|warn|error
workhive log --clear                # 清空日志文件
```

| 选项 | 说明 |
|------|------|
| `--follow` | 实时跟踪日志 |
| `--lines <N>` | 显示行数（默认: 50）|
| `--clear` | 清空日志文件 |
| `--level <level>` | 按级别过滤：`debug` / `info` / `warn` / `error` |
| `-h, --help` | 显示帮助 |

---

### `workhive shell` — Shell 集成

统一管理 Shell 集成（最小化默认 + 模块化附加），并在 rc 文件中只保留一条托管 `source` 入口。

```bash
workhive shell setup                                 # 交互式配置
workhive shell setup --shell zsh --profile minimal  # 最小化配置
workhive shell setup --profile standard --enable zoxide --yes
workhive shell init --shell zsh                     # setup 别名
workhive shell remove                # 移除集成
workhive shell show                  # 输出集成脚本
```

> 交互模式会先显示 `minimal / standard / advanced` 三档选择（1/2/3），随后可按需启用或禁用模块。
> 当传入 `--enable` 或 `--disable` 时，会直接按参数生效，跳过“逐项自定义模块”交互。

| 子命令 | 说明 |
|--------|------|
| `setup` | 按 profile + 模块生成并注入 shell 集成 |
| `init` | `setup` 的别名 |
| `remove` | 从配置文件中移除 Shell 集成 |
| `show` | 将集成脚本输出到 stdout |

| 选项 | 说明 |
|------|------|
| `--shell <shell>` | 目标 Shell：`zsh`、`bash`（默认：自动检测） |
| `--profile <name>` | 配置档位：`minimal` / `standard` / `advanced` |
| `--enable <csv>` | 额外启用模块：`completions,aliases,history,zoxide,starship,ssh-term-fix`（传入后跳过逐项模块交互） |
| `--disable <csv>` | 从档位中禁用模块（传入后跳过逐项模块交互） |
| `--yes` | 非交互模式（默认 `minimal`） |
| `--dry-run` | 预览变更，不修改文件 |
| `-h, --help` | 显示帮助 |

> Shell 托管脚本位于 `~/.config/workhive/shell/workhive.{zsh,bash}`，`setup` 会维护该文件并在 rc 文件中写入单一 `source` 入口。

---

### `workhive upgrade` — 检查更新并升级

从 GitHub 获取最新版本并更新 workhive。

```bash
workhive upgrade                     # 升级到最新版本
workhive upgrade --check             # 仅检查是否有更新
workhive upgrade --version v0.3.0    # 安装指定版本
```


| 选项                | 说明             |
| ----------------- | -------------- |
| `--check`         | 仅检查更新，不安装      |
| `--version <tag>` | 安装指定版本         |
| `--force`         | 强制重新安装（即使已是最新） |
| `-h, --help`      | 显示帮助信息         |


**自动检测安装方式:**

- **Git clone** — 使用 `git pull` 更新
- **安装脚本** — 重新执行 `install.sh`
- **开发模式** — 显示手动更新指引

> 说明: `upgrade --check` 需要可访问 GitHub。离线或受限网络环境下会提示无法检查远程版本。

---

### 全局选项

```bash
workhive --version               # 显示版本信息
workhive --help                  # 显示帮助信息
workhive --debug <command>       # 启用调试输出
```

---

## 最佳实践

### 快捷键速查

#### tmux 内置快捷键

所有 workhive 工作空间都运行在 tmux 中。tmux 的**前缀键**是 `Ctrl+B`（先按 Ctrl+B，再按后续按键）。


| 快捷键              | 操作                        |
| ---------------- | ------------------------- |
| `Ctrl+B d`       | **分离**会话 (后台运行, SSH 断线安全) |
| `Ctrl+B o`       | 切换到**下一个** Pane (循环切换)    |
| `Ctrl+B ;`       | 切换到**上一个** Pane           |
| `Ctrl+B ↑/↓/←/→` | 切换到指定**方向**的 Pane         |
| `Ctrl+B z`       | **最大化/恢复**当前 Pane (缩放切换)  |
| `Ctrl+B [`       | 进入**滚动/复制模式** (按 `q` 退出)  |
| `Ctrl+B x`       | 关闭当前 Pane (需确认)           |


#### workhive 增强快捷键

workhive 在创建会话时自动注入以下快捷键 (需要 tmux 3.3+):


| 快捷键        | 操作                       | 依赖      |
| ---------- | ------------------------ | ------- |
| `Ctrl+B g` | 弹出浮动 Lazygit 窗口 (80% 屏幕) | Lazygit |
| `Ctrl+B y` | 弹出浮动 yazi 文件浏览器 (80% 屏幕) | yazi    |
| `Ctrl+B n` | 弹出当前工作空间笔记编辑器 | — |
| `Ctrl+B w` | 附加到另一个工作空间 | — |
| `Ctrl+B L` | 切换 Pane 布局（交互式菜单） | — |

这些 Popup 窗口悬浮于当前 Pane 之上，按 `Esc` 或 `q` 关闭后自动恢复原布局。适合在 Claude Code 对话中临时查看 Git 状态、浏览文件或记录笔记，无需切换 Pane。

> 如果 tmux 版本低于 3.3 或未安装对应工具，快捷键不会被注入，不影响正常使用。

### Pane 切换场景

**场景：在三分屏布局中切换 Pane**

```
┌─────────────┬──────────────┐
│ Claude Code  │              │
│ (Pane 0)     │  Lazygit     │
│              │  (Pane 2)    │
├─────────────┤              │
│ Shell        │              │
│ (Pane 1)     │              │
└─────────────┴──────────────┘
```

- Claude Code → Lazygit: `Ctrl+B →`（右箭头）
- Lazygit → Claude Code: `Ctrl+B ←`（左箭头）
- Claude Code → Shell: `Ctrl+B ↓`（下箭头）
- Shell → Claude Code: `Ctrl+B ↑`（上箭头）
- 循环切换所有 Pane: 反复按 `Ctrl+B o`

**场景：临时最大化某个 Pane**

当你需要全屏查看 Claude Code 或 Lazygit 时：

1. 按 `Ctrl+B z` **放大**当前 Pane 为全屏
2. 完成工作
3. 再按 `Ctrl+B z` **恢复**原布局

### 常用操作

**场景：在工作空间内关闭一个 Pane**

```bash
# 方法 1: 使用 workhive 命令 (在 tmux 内)
workhive pane close                  # 需确认
workhive pane close -f               # 跳过确认 (AI 自动化)

# 方法 2: 使用 tmux 快捷键
# Ctrl+B x → 按 'y' 确认

# 方法 3: 退出 Pane 中的进程
# 例如在 Shell 中输入 'exit'，或在 Lazygit 中按 'q'
```

> 注意：不能关闭会话中的最后一个 Pane。使用 `workhive rm <worktree>` 关闭整个工作空间。

**场景：在当前 Pane 中打开新工具**

```bash
# 交互式: 选择 Worktree 再选择命令
workhive pane open

# 直接指定 worktree
workhive pane open feature-x

# 非交互式: 指定 worktree + 命令 (AI 自动化)
workhive pane open feature-x --cmd claude

# 自动创建 worktree + 打开工具 (完全非交互式)
workhive pane open feature-x --cmd claude -f
```

**场景：在当前 Pane 下方新增一个 Pane**

```bash
# 空 Shell
workhive pane split

# 指定命令
workhive pane split --cmd "npm run dev"
workhive pane split --cmd lazygit

# 分割 + 为任务分支打开 AI 工具 (多任务常用)
workhive pane split --cmd "workhive pane open task-auth --cmd claude -f"
```

**场景：向其他 Pane 发送文本 (AI 自动化)**

```bash
# 向 pane 1 发送命令
workhive pane send 1 "npm test"

# 发送文本不按回车
workhive pane send 2 "hello" --commit none

# 向指定 pane 集合同时发送
workhive pane send 1,2,3 "npm test"

# 向当前会话所有 pane 广播同一条文本
workhive pane send --all "npm test"

# 查看 pane 索引
workhive pane list

# 查看当前 pane 详情
workhive pane current
```

**场景：修改布局配置**

```bash
workhive config --layout
workhive config --global --layout
```

### 工作流技巧

1. **不要手动分割 tmux Pane** — 让 workhive 管理布局。如需更多 Pane，使用 `workhive pane split`；如需调整布局，使用 `workhive config --layout` 进行交互式修改。
2. **批量创建使用 `--no-attach`** — 一次设置多个工作空间：
  ```bash
   workhive open feature-a --no-attach
   workhive open feature-b --no-attach
   workhive attach feature-a  # 进入你想要的那个
  ```
3. **3 秒恢复 SSH 会话** — 重新连接 SSH 后：
  ```bash
   workhive ls              # 看看有什么在跑
   workhive attach feature-x  # 接着干
  ```
4. **版本化你的配置** — 将 `.workhive/config.yaml` 提交到仓库，让团队共享同一套环境配置。
5. **用弹窗快捷键处理临时任务** — `Ctrl+B g` 查看 Git，`Ctrl+B y` 浏览文件，`Ctrl+B w` 切换工作空间——无需离开当前 Pane。

---

## 配置文件

配置文件位于 `.workhive/config.yaml`，由 `workhive init` 生成，也可手动编辑。
完整配置参考（中英双语）：
- [help/reference/configuration.zh-CN.md](help/reference/configuration.zh-CN.md)（中文）
- [help/reference/configuration.md](help/reference/configuration.md)（English）

### 配置优先级

1. **项目级** — `.workhive/config.yaml` (最高优先级)
2. **用户级** — `~/.config/workhive/config.yaml`（或 `$XDG_CONFIG_HOME/workhive/config.yaml`）
3. **内置默认值** (最低优先级)

### 完整配置参考

`config.yaml` 中所有可用的配置键如下。所有键均为可选——未指定的键使用内置默认值。
`workhive init` 会写入这套配置结构；`workhive config` 仅做严格校验，不会回填旧键。

| 键 | 类型 | 默认值 | 说明 |
|----|------|--------|------|
| `version` | string | (当前版本) | 配置文件版本（由 `workhive init` 自动设置）|
| `language` | string | `"en"` | 界面语言：`"en"`（英文）或 `"zh"`（中文）|
| **布局 (Column-based DSL)** | | | |
| `layout.columns` | array | (2列默认) | 列定义数组 |
| `layout.columns[].width` | int | 50 | 列宽度百分比 |
| `layout.columns[].panes` | array | — | 该列的面板定义数组 |
| `layout.columns[].panes[].height` | int | 50 | 面板高度百分比 |
| `layout.columns[].panes[].command` | string\|null | null | 面板命令 |
| `layout.columns[].panes[].role` | string\|null | null | 可选面板角色（用于角色寻址）|
| **Worktree** | | | |
| `worktree.base_path` | string | `".workhive/worktrees"` | Worktree 存放目录（相对于 Git 根目录）|
| `worktree.auto_create` | bool | `true` | `open` 时自动创建 Worktree |
| `worktree.auto_delete` | bool | `true` | `remove` 时自动删除 Worktree |
| `worktree.sync_files` | array | `[]` | 新 Worktree 同步的文件列表 |
| `worktree.sync_files[].source` | string | — | 源文件/目录路径（相对于 Git 根目录）|
| `worktree.sync_files[].mode` | string | `"copy"` | 同步模式：`"copy"` 或 `"symlink"` |
| **tmux** | | | |
| `tmux.session_prefix` | string\|null | null | 自定义会话名前缀（null = 自动：`wh@<id>@<项目名>`）|
| `tmux.mouse_support` | bool | `true` | 启用 tmux 鼠标支持 |
| `tmux.env` | map | `{ CLAUDE_CODE_NO_FLICKER: 1 }` | 注入到 pane 启动命令的环境变量（优先级：项目 > 全局） |
| `tmux.theme` | string | `"catppuccin-mocha"` | 状态栏主题（见下方可选主题）|
| `tmux.attach_delay` | duration | `0s` | 自动进入会话前的可选等待时长；未设置时默认立即 attach |
| **快照恢复** | | | |
| `tmux.persistence.enabled` | bool | `false` | 启用快照保存/恢复 |
| `tmux.persistence.generation` | int | `0` | 快照代际隔离标记；重新启用快照恢复时自动递增 |
| `tmux.persistence.auto_save_mode` | string | `"hybrid"` | 自动保存策略：`hybrid`（事件+定时）、`event`（仅事件）、`interval`（仅定时） |
| `tmux.persistence.auto_save_debounce` | duration | `3s` | 事件触发保存的防抖窗口 |
| `tmux.persistence.auto_save_interval` | duration | `10m` | `interval`/`hybrid` 模式的定时保存间隔 |
| `tmux.persistence.auto_save_env` | map | `{ WORKHIVE_SAVE_LOCK_QUIET: 1, WORKHIVE_SAVE_QUALITY_QUIET: 1 }` | 注入到内部自动保存流程的环境变量；仅使用显式配置值 |
| `tmux.persistence.pre_kill_env` | map | `{ WORKHIVE_SAVE_LOCK_QUIET: 1, WORKHIVE_SAVE_QUALITY_QUIET: 1, WORKHIVE_SAVE_QUALITY_MAX_RETRIES: 1 }` | 注入到关闭前快照保存流程的环境变量；仅使用显式配置值 |
| `tmux.persistence.restore_processes` | bool | `true` | 恢复时重启面板中的进程 |
| `tmux.persistence.snapshot_history_limit` | int | `8` | 历史归档快照数量上限（不含 `wh_snapshot_latest.jsonl` 及其目标文件） |
| `tmux.persistence.snapshot_max_size_mb` | int | `8` | 快照归档总容量上限（MB，`0`=不限制） |
| `maintenance.temp_cleanup.max_age_days` | int | `7` | 自动清理超过 N 天的陈旧临时配置文件 |
| `maintenance.log.path` | string | `".workhive/logs/workhive.log"` | 运行时文件日志路径（绝对路径，或相对 Git 根目录） |
| `maintenance.test_log.path` | string | `"/dev/null"` | 测试模式文件日志路径（`/dev/null` 表示默认不落测试日志文件） |
| `maintenance.test_log.level` | string | 继承 `maintenance.log.level` | 可选测试日志级别覆盖：`debug`、`info`、`warn`、`error` |
| `maintenance.test_log.format` | string | 继承 `maintenance.log.format` | 可选测试日志格式覆盖：`text` 或单行 `json` |
| `maintenance.test_log.max_size_mb` | int | 继承 `maintenance.log.max_size_mb` | 可选测试日志单文件轮转阈值（MB） |
| `maintenance.test_log.max_backups` | int | 继承 `maintenance.log.max_backups` | 可选测试日志轮转保留份数（`0` = 截断模式） |
| `maintenance.log.level` | string | `"info"` | 文件日志级别阈值：`debug`、`info`、`warn`、`error` |
| `maintenance.log.format` | string | `"text"` | 文件日志格式：`text` 或单行 `json` |
| `maintenance.log.max_size_mb` | int | `10` | 单文件日志轮转阈值（MB） |
| `maintenance.log.max_backups` | int | `5` | 日志轮转保留份数（`0` = 截断模式） |
| **Git 初始化安全守卫（仅全局）** | | | |
| `git_init.whitelist_paths` | string[] | `[]` | 允许自动 `git init` 的目录白名单（含子目录）；非空时仅白名单内目录可触发初始化 |
| `git_init.dangerous_paths_extra` | string[] | *系统相关* | 额外禁止 `git init` 的目录列表（仅精确匹配）；默认根据操作系统自动生成（`/`、`~`、系统目录等） |
| **工具注册表** | | | |
| `tools.default` | string | `"__empty__"` | 默认工具引用（`tool_name` 或特殊值 `__empty__`、`__custom__`） |
| `tools.list` | array | 内置列表 | 面板工具注册表列表 |
| `tools.list[].tool_name` | string | — | 工具名称（全局唯一） |
| `tools.list[].label` | string | `tool_name` | `workhive tools list` 中展示的标签 |
| `tools.list[].command` | string | — | 实际执行命令 |
| `tools.list[].is_ai` | bool | `false` | 是否标记为 AI 工具（用于 AI 检测/筛选） |
| `tools.list[].enabled` | bool | `true` | 是否在可选列表中启用 |

#### 字段取值说明（可填写什么）

| 字段 | 可填写值/格式 | 含义 |
|------|---------------|------|
| `tmux.persistence.enabled` | `true` / `false` | 启用/禁用快照保存与恢复 |
| `tmux.persistence.generation` | 整数 `>=0` | 快照代际隔离标记；`enabled` 从 `false` 切到 `true` 时自动递增 |
| `tmux.persistence.auto_save_mode` | `hybrid` / `event` / `interval` | `hybrid`=事件+定时，`event`=仅事件，`interval`=仅定时 |
| `tmux.persistence.auto_save_debounce` | duration：`3s`、`30s`、`1m`、`2h` | 事件触发保存的防抖窗口 |
| `tmux.persistence.auto_save_interval` | duration：`1m`、`10m`、`1h` | `interval/hybrid` 的定时保存间隔 |
| `tmux.persistence.restore_processes` | `true` / `false` | 恢复时是否尝试重启 pane 内进程 |
| `tmux.persistence.snapshot_history_limit` | 整数 `>=1` | 历史归档快照保留数量上限（不含 latest 指针及其目标文件） |
| `tmux.persistence.snapshot_max_size_mb` | 整数 `>=0` | 快照归档总容量上限（MB）；`0`=不按容量限制 |
| `maintenance.log.path` | 绝对路径或相对 Git 根目录路径 | 运行时日志输出文件路径 |
| `maintenance.test_log.path` | 绝对路径或相对 Git 根目录路径 | 测试模式日志输出文件路径（显式配置时生效） |
| `maintenance.test_log.level` | `debug` / `info` / `warn` / `error` | 可选测试日志级别覆盖；未配置时继承 `maintenance.log.level` |
| `maintenance.test_log.format` | `text` / `json` | 可选测试日志格式覆盖；未配置时继承 `maintenance.log.format` |
| `maintenance.test_log.max_size_mb` | 整数 `>=1` | 可选测试日志轮转阈值；未配置时继承 `maintenance.log.max_size_mb` |
| `maintenance.test_log.max_backups` | 整数 `>=0` | 可选测试日志保留份数；未配置时继承 `maintenance.log.max_backups` |
| `maintenance.log.level` | `debug` / `info` / `warn` / `error` | 写入文件日志的最低级别 |
| `maintenance.log.format` | `text` / `json` | 文件日志输出格式 |
| `maintenance.log.max_size_mb` | 整数 `>=1` | 单文件轮转阈值（达到后触发轮转） |
| `maintenance.log.max_backups` | 整数 `>=0` | 保留的轮转备份份数（`0` = 截断模式） |
| `tmux.attach_delay` | duration：`0s`、`1s`、`2s`、`5s` | 自动进入会话前的可选延迟 |
| `tools.default` | `tool_name` / `__empty__` / `__custom__` | 默认工具选择（`__empty__`=空 Shell，`__custom__`=提示输入自定义命令） |

#### 日志路径默认值（运行日志 vs 测试日志）

workhive 默认将运行日志与测试日志分离，并支持配置：

```yaml
maintenance:
  log:
    path: ".workhive/logs/workhive.log"
    level: info
    format: text
    max_size_mb: 10
    max_backups: 5
  test_log:
    path: "/dev/null"   # 如需保留独立测试日志，改为 ".workhive/logs/workhive-test.log"
```

说明：
- 两个路径都支持绝对路径或相对 Git 根目录路径。
- `workhive init` 初始化时会默认写入这两个键，方便后续手动修改。
- 若未配置 `maintenance.test_log.level/format/max_size_mb/max_backups`，会继承 `maintenance.log.*`。
- pane 运行时的快照质量/锁竞争提示默认静默（但仍会写入文件日志）。

> **命令值:** 使用 `null` 表示空 Shell，或使用引号字符串如 `"claude --dangerously-skip-permissions"` 指定工具。命令的第一个词必须是已安装的可执行文件。

> **可选主题:** `catppuccin-mocha` · `tokyo-night` · `dracula` · `nord` · `gruvbox` · `rose-pine` · `everforest` · `onedark`

> `tools` 仅支持全局配置，建议使用 `workhive tools ...` 管理。
> `git_init.*` 与 `tools.*` 仅写入全局配置；项目配置不会持久化这些键。
> 推荐命令：`workhive tools list|add|edit|remove|set-default|validate|manage`。

#### Git 初始化安全守卫

workhive 在非 Git 仓库目录下运行时，可以协助自动执行 `git init`。为防止在危险目录意外初始化 Git 仓库，workhive 提供了多层安全防护：

**评估策略（按优先级执行）：**
1. **内置硬保护** — 根目录 `/` 始终硬阻断，不可覆盖
2. **`whitelist_paths`** — 若非空，白名单中的目录（及其子目录）将立即放行
3. **`dangerous_paths_extra`** — 用户可配置的精确匹配阻断目录（执行 `workhive init --global` 时会自动填充主目录/系统目录默认值）
4. **用户确认** — 通过以上检查的目录，仍需用户显式确认（输入 `y`）才会执行初始化

**配置示例：**

```yaml
# 全局配置 (~/.config/workhive/config.yaml)
git_init:
  whitelist_paths:
    - "/Users/me/projects"
    - "/Users/me/work"
  dangerous_paths_extra:
    - "/tmp"
    - "/var"
    - "/usr"
    # ... 更多系统目录（初始化时自动生成）
```

**使用 CLI 管理白名单：**

```bash
workhive config git-whitelist add ~/projects      # 添加白名单目录
workhive config git-whitelist add ~/work           # 添加另一个目录
workhive config git-whitelist remove ~/projects    # 移除白名单目录
workhive config git-whitelist list                 # 查看当前白名单
```

> **提示：** 根目录 `/` 由内置硬保护处理。`dangerous_paths_extra` 会在执行 `workhive init --global` 时自动填充默认危险目录；macOS 默认包含主目录、`/System`、`/Applications`、`/Library` 及用户文件夹，Linux 默认包含主目录及 `/boot`、`/sbin`、`/proc`、`/sys` 等。你可以在全局配置文件中手动编辑此列表。
> 若同一路径同时出现在白名单与 `dangerous_paths_extra` 中，白名单优先；唯一不可覆盖的是根目录 `/` 的硬保护。

### 布局配置 (Column-based DSL)

workhive 使用统一的 **Column-based DSL** 定义所有布局。每个布局由列数组组成，每列拥有独立的宽度和面板定义。

#### 宽高比例

`width` 和 `height` 的值为**百分比** (0-100)：


| 属性                         | 含义            | 作用范围 |
| -------------------------- | ------------- | ---- |
| `columns[].width`          | 列宽度占总宽度的百分比   | 每列   |
| `columns[].panes[].height` | 面板高度占所在列高度的百分比 | 每面板  |


> **操作步骤**: 修改 `.workhive/config.yaml` 中的 `width`/`height` 值后，使用 `-f/--force` 强制按配置重建：
>
> ```bash
> workhive open <branch> -f    # 销毁旧会话并按最新配置重建
> ```
>
> 其他方式：
>
> ```bash
> workhive config --layout
> workhive remove <branch>     # 或者：关闭后重新打开
> workhive open <branch>
> ```

#### AI Coding 布局 (2列, 2+1 面板)

```yaml
layout:
  columns:
    - width: 60
      panes:
        - height: 50
          command: "claude --dangerously-skip-permissions"
        - height: 50
          command: null                    # 空 Shell
    - width: 40
      panes:
        - height: 100
          command: "lazygit"
```

#### Quad 布局 (2列, 2+2 面板)

```yaml
layout:
  columns:
    - width: 60
      panes:
        - height: 50
          command: "claude --dangerously-skip-permissions"
        - height: 50
          command: null
    - width: 40
      panes:
        - height: 70
          command: "lazygit"
        - height: 30
          command: "yazi"
```

#### Multi-Agent 布局 (2列, 4+1 面板, 带角色)

```yaml
layout:
  columns:
    - width: 50
      panes:
        - height: 25
          command: "claude --dangerously-skip-permissions"
          role: "agent1"
        - height: 25
          command: "claude --dangerously-skip-permissions"
          role: "agent2"
        - height: 25
          command: "claude --dangerously-skip-permissions"
          role: "agent3"
        - height: 25
          command: "claude --dangerously-skip-permissions"
          role: "agent4"
    - width: 50
      panes:
        - height: 100
          command: null
          role: "preview"
```

> **面板角色** 支持按名称寻址面板：`workhive pane send --role preview "preview path/to/file"`

#### 3列布局

```yaml
layout:
  columns:
    - width: 33
      panes:
        - height: 50
          command: "claude --dangerously-skip-permissions"
        - height: 50
          command: null
    - width: 34
      panes:
        - height: 100
          command: "lazygit"
    - width: 33
      panes:
        - height: 50
          command: "npm run dev"
        - height: 50
          command: "npm test -- --watch"
```

#### 单列布局 (1列, 面板垂直堆叠)

```yaml
layout:
  columns:
    - width: 100
      panes:
        - height: 33
          command: "claude --dangerously-skip-permissions"
        - height: 33
          command: null
        - height: 34
          command: "lazygit"
```

### Worktree 配置

```yaml
worktree:
  base_path: ".workhive/worktrees"     # Worktree 存放路径
  auto_create: true                    # open 时自动创建
  auto_delete: true                    # remove 时自动删除
  sync_files:                          # 新建 worktree 时自动同步的文件
    - source: ".env"
      mode: "copy"                     # copy: 复制文件
    - source: "node_modules"
      mode: "symlink"                  # symlink: 创建符号链接
```

通过 `workhive open` 创建新 worktree 时，`sync_files` 中列出的文件会自动从项目根目录复制或符号链接到新目录。确保环境文件 (`.env`)、依赖 (`node_modules`) 等未被 git 跟踪的文件即刻可用。

### tmux 配置

```yaml
tmux:
  # session_prefix: null               # 运行时自动计算: wh@<instance_id>@<项目名>
  mouse_support: true                  # 鼠标支持
  attach_delay: 0s                      # 自动 attach 前等待秒数
  theme: "catppuccin-mocha"            # 主题
```

**会话命名:** workhive 会为每个项目自动生成 4 位十六进制 instance ID（存储在 `.workhive/instance_id`）。会话名格式为 `wh@<id>@<项目名>@<分支名>`，确保不同项目即使目录同名也能隔离。如需覆盖，取消 `session_prefix` 注释并填入自定义值。

可选主题: `catppuccin-mocha` | `tokyo-night` | `dracula` | `nord` | `gruvbox` | `rose-pine` | `everforest` | `onedark`

### 会话快照恢复

workhive 内置 tmux 快照恢复模块，可自动保存和恢复 tmux 会话结构。快照捕获窗口、面板、布局、工作目录和运行中的命令。支持**项目级**和**全局**两种配置。

> 该功能专为 tmux 设计，因此配置放在 `tmux.persistence` 命名空间下。

#### 启用快照恢复

```bash
# 方式一：交互式配置
workhive config              # 选择 "Session Persistence"（快照恢复）→ Yes

# 方式二：命令行直接设置（项目级）
workhive config set tmux.persistence.enabled true

# 方式三：全局启用（对所有项目生效）
workhive config set tmux.persistence.enabled true --global

# 在初始化时启用
workhive init                # 步骤 4 会询问是否启用
```

#### 配置参考（`config.yaml`）

```yaml
tmux:
  persistence:
    enabled: true               # 启用/禁用快照保存与恢复
    generation: 0               # 快照代际隔离标记（通常无需手动修改）
    auto_save_mode: hybrid      # 自动保存策略: hybrid / event / interval
    auto_save_debounce: 3s  # 事件保存防抖（秒）
    auto_save_interval: 10m       # 定时保存间隔（分钟，hybrid/interval 共用）
    restore_processes: true     # 恢复时是否重启面板中的进程
    process_list: [":all:"]     # 捕获并恢复的进程名单 (如 ["vi", "npm", "hermes"])。":all:" 代表所有进程。

    snapshot_history_limit: 8   # 历史归档快照数量上限（不含 latest 指针及其目标文件）
    snapshot_max_size_mb: 8     # 归档快照总容量上限（MB）
```

#### 字段取值说明（可填写什么）

| 字段 | 类型 | 可填写值/格式 | 含义 |
|------|------|---------------|------|
| `tmux.persistence.enabled` | bool | `true` / `false` | 是否启用快照保存与恢复 |
| `tmux.persistence.generation` | int | `0` 或更大整数 | 快照代际隔离。`enabled` 从 `false` 切到 `true` 时会自动递增；通常不建议手改 |
| `tmux.persistence.auto_save_mode` | enum | `hybrid` / `event` / `interval` | `hybrid`=事件+定时，`event`=仅事件，`interval`=仅定时 |
| `tmux.persistence.auto_save_debounce` | duration | 如 `3s`、`30s`、`1m`、`2h` | 事件触发保存的防抖窗口，仅事件路径生效 |
| `tmux.persistence.auto_save_interval` | duration | 如 `1m`、`10m`、`1h` | 定时保存间隔，`interval/hybrid` 模式生效 |
| `tmux.persistence.restore_processes` | bool | `true` / `false` | 恢复会话时是否尝试重启 pane 内进程 |
| `tmux.persistence.process_list` | array | `[":all:"]` / `["vi", "npm", "hermes"]` | 配置哪些进程在保存快照时被捕获和恢复。默认恢复所有。 |

| `tmux.persistence.snapshot_history_limit` | int | `>=1` | 历史归档快照保留数量上限（不含 `wh_snapshot_latest.jsonl` 及其目标文件） |
| `tmux.persistence.snapshot_max_size_mb` | int | `>=0` | 快照归档总容量上限（MB）；`0` 表示不限制大小 |

补充：
- duration 目前支持 `s` / `m` / `h` 单位（秒/分钟/小时）。
- `tmux.attach_delay` 也是 duration，常用值：`0s`、`1s`、`2s`、`5s`。

#### 保存内容

| 项目 | 保存？ | 说明 |
|------|--------|------|
| 窗口和面板 | ✅ 是 | 布局、大小、位置 |
| 工作目录 | ✅ 是 | 每个面板的当前目录 |
| 面板命令 | ✅ 是 | 运行中的命令（lazygit、claude 等）|
| 环境变量 | ❌ 否 | 不保存 shell 环境变量 |
| 面板内容/历史 | ❌ 否 | 终端滚动内容不会保存到快照 |

#### 快照存储

快照按项目存储在 `.workhive/snapshots/` 目录中。每个快照是一个 JSONL 格式文件（每行一个 JSON 对象）：

- `wh_snapshot_latest.jsonl` 指向最新快照
- 历史归档使用 `wh_snapshot_archive_<timestamp>.jsonl`
- `snapshot_history_limit` 仅统计历史归档，不包含 `wh_snapshot_latest.jsonl` 及其目标文件

#### 工作原理

```
workhive open/attach
    │
    ├── persistence.enabled = false?
    │   └── 按 config.yaml 布局创建会话 (标准流程)
    │
    └── persistence.enabled = true?
        ├── 有快照 
        │   └── 从快照恢复：重建窗口/面板/布局/工作目录
        │       └── restore_processes = true? → 重启可恢复进程
        │
        └── 无快照?
            └── 按 config.yaml 布局创建会话 (标准流程)

会话运行中:
    ├── auto_save_mode=event: 仅在 pane/window/layout 等事件变化时保存
    ├── auto_save_mode=interval: 按 auto_save_interval 定时保存
    ├── 事件触发保存受 auto_save_debounce 防抖保护
    ├── 用户 detach 时保存快照
    ├── workhive close 前保存快照
    └── auto_save_mode=hybrid: 事件触发 + 定时保存 (auto_save_interval)

快照清理:
    ├── 按 snapshot_history_limit 裁剪历史数量
    └── 按 snapshot_max_size_mb 裁剪总容量
```

**关键点：**
- 快照保存的是 tmux 会话结构（窗口、面板布局、工作目录、运行命令），不是终端内容
- 恢复时，面板会被重建到相同的工作目录，可恢复的进程会被重新启动
- 默认可恢复进程列表包括：`vim`、`nvim`、`lazygit`、`yazi`、`htop`、`top` 等
- 如果恢复失败，会自动回退到 `config.yaml` 中定义的布局

#### 查看快照恢复状态

```bash
# 在 list 命令中查看
workhive list          # 底部会显示快照恢复状态、最近快照时间

# 在 doctor 命令中检查
workhive doctor        # 显示快照模块健康状态、快照数量、最近快照

# 查看快照配置
workhive config        # 显示 enabled/auto_save_interval
```

#### 如何设置（可直接复制）

```bash
# 1) 启用快照恢复
workhive config set tmux.persistence.enabled true

# 2) 推荐的小体积配置（适合大多数项目）
workhive config set tmux.persistence.snapshot_history_limit 8
workhive config set tmux.persistence.snapshot_max_size_mb 8

# 3) 推荐自动保存策略（高可用 + 低写入）
workhive config set tmux.persistence.auto_save_mode hybrid
workhive config set tmux.persistence.auto_save_debounce 3s
workhive config set tmux.persistence.auto_save_interval 10m
```

> 如果你希望“完全只在变化时保存”，可设为：`tmux.persistence.auto_save_mode=event`。

#### 禁用快照恢复

```bash
workhive config set tmux.persistence.enabled false
```

### 场景配置示例

AI 辅助开发 (Claude + Shell + Lazygit)

```yaml
layout:
  columns:
    - width: 60
      panes:
        - height: 50
          command: "claude --dangerously-skip-permissions"
        - height: 50
          command: null
    - width: 40
      panes:
        - height: 100
          command: "lazygit"
```

---

## 典型工作流

### 并行开发多个分支

```bash
workhive open feature-x          # 终端 1: 开发 feature-x
workhive open feature-y          # 终端 2: 同时开发 feature-y (完全隔离)
workhive ls                      # 查看所有空间
workhive attach feature-y        # 切换到另一个空间
workhive rm feature-x            # 完成后销毁
```

### SSH 远程会话恢复

```bash
# 本地创建开发环境
workhive open feature-x
# Ctrl+B d 分离会话 (或直接断开 SSH)

# 之后重新 SSH 连入
ssh server && cd project
workhive attach feature-x        # 一切都在，原封不动
```

### 远程救火 (手机 SSH 场景)

```bash
# 手机 SSH 连入服务器
ssh server && cd project
workhive ls                       # 看看有哪些空间
workhive attach my-service        # 重连之前运行中的会话
# Claude Code 还在跑，直接继续
```

---

## 环境变量


| 变量                          | 说明                          | 默认值    |
| --------------------------- | --------------------------- | ------ |
| `WORKHIVE_DEBUG`            | 启用调试输出                      | 未设置    |
| `WORKHIVE_PLATFORM`         | 覆盖平台检测                      | 自动检测   |
| `WORKHIVE_TERMINAL_ADAPTER` | 强制终端适配器 (`tmux` / `screen`) | `auto` |


```bash
WORKHIVE_DEBUG=1 workhive open feature-x   # 启用调试
workhive --debug open feature-x            # 同上
```

---

## 项目架构

workhive 采用**分层架构**，关注点分离清晰：

```
bin/workhive                       # CLI 入口 & 命令分发
├── lib/app/commands/              # 应用层: 15 个公开命令
│                                    (init, open, attach, close, pane, list, remove,
│                                     config, doctor, gc, destroy, rename, upgrade,
│                                     log, shell)
├── lib/app/shared/                # 共享层: worktree 收集器/选择器/安全检查、
│                                    命令构建器、主题助手、布局可视化/重建、帮助索引
├── lib/domain/services/           # 领域层: worktree, session, layout, workspace
├── lib/infra/                     # 基础设施层
│   ├── adapters/terminal/         #   终端适配器 (tmux / screen)
│   ├── ai/                        #   AI 工具适配器 (claude, codex, opencode)
│   ├── cache/                     #   内存缓存 (TTL, 批量失效)
│   ├── config/                    #   配置加载器 (3 层优先级)
│   ├── persistence/               #   会话快照恢复 (快照保存/恢复/自动保存/进程捕获)
│   ├── platform/                  #   跨平台检测 & 路径处理
│   └── tools/                     #   工具注册表 (窗格工具管理)
└── lib/utils/                     # 工具层: 日志, TUI, i18n, 公共函数
```

**设计模式:**

- **Adapter Pattern** — tmux/screen 适配器接口
- **Factory Pattern** — 基于平台自动选择适配器
- **Source Guard** — 防止重复加载
- **3-Tier Config** — 项目 → 用户 → 默认值优先级

---

## 测试

workhive 包含集成测试和单元测试，确保核心功能正确性。

### 运行测试

```bash
# 运行所有单元测试
bash test/unit/run_all.sh

# 运行所有集成测试 (需要 tmux)
bash test/integration/full_workflow_test.sh
bash test/integration/domain_services_test.sh
bash test/integration/non_worktree_test.sh

# BDD 测试
bash test/integration/list_bdd_test.sh
bash test/integration/new_features_bdd_test.sh
bash test/integration/comprehensive_bdd_test.sh
bash test/integration/reliability_bdd_test.sh
bash test/integration/pane_layout_bdd_test.sh
bash test/integration/config_command_test.sh
bash test/integration/robustness_bdd_test.sh
bash test/integration/new_commands_bdd_test.sh
```

### 测试覆盖


| 测试套件                                        | 覆盖范围                                       |
| ------------------------------------------- | ------------------------------------------ |
| `unit/`                                     | 所有工具函数、缓存、配置、平台检测、路径处理、i18n、JSON 转义、Source Guard、版本 |
| `unit/test_commands.sh`                     | CLI 参数解析、帮助信息、版本号                          |
| `unit/test_list.sh`                       | List 仪表盘命令: 数据收集、格式化、Git 状态             |
| `unit/test_new_commands.sh`                 | doctor、gc 命令、会话通知及快捷键                       |
| `unit/test_persistence.sh`                  | 快照恢复模块: 快照保存/恢复、自动保存、清理、配置解析               |
| `integration/full_workflow_test.sh`         | init → open → list → attach → remove 端到端流程 |
| `integration/domain_services_test.sh`       | Worktree + Session + Layout 服务集成           |
| `integration/non_worktree_test.sh`          | 非 Worktree 单目录模式                           |
| `integration/list_bdd_test.sh`            | List 仪表盘命令 BDD 场景                      |
| `integration/new_features_bdd_test.sh`      | doctor、gc、命令面与布局相关 BDD 场景                 |
| `integration/comprehensive_bdd_test.sh`     | 跨功能集成场景                                    |
| `integration/reliability_bdd_test.sh`       | 错误处理、边界情况、恢复                               |
| `integration/pane_layout_bdd_test.sh`       | Pane 和布局管理                                 |
| `integration/config_command_test.sh`        | Config 命令集成                                |
| `integration/robustness_bdd_test.sh`        | 配置回退、哨兵保护、初始化错误处理                          |
| `integration/new_commands_bdd_test.sh`      | rename、log 命令                                |

---

## 贡献指南

欢迎贡献! 请遵循以下规范:

1. **Fork** 本仓库并创建特性分支
2. 确保代码通过 `shellcheck` 检查
3. 为新功能编写测试
4. 提交 Pull Request 并描述变更内容

### 开发环境

```bash
git clone https://github.com/yboker/workhive.git
cd workhive

# 运行测试验证环境
bash test/unit/run_all.sh
bash test/integration/full_workflow_test.sh
```

### 代码规范

- 遵循 [Google Shell Style Guide](https://google.github.io/styleguide/shellguide.html)
- 使用 `set -euo pipefail` 确保错误处理
- 所有公共函数需要文档注释
- 变量使用 `local` 声明，避免全局污染

---

## License

[MIT](LICENSE)
