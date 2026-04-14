# workhive — 快速上手指南

> 一条命令编排 pane、命令与会话。

[English](README.md) | [中文](README.zh-CN.md)

---

## 概述

workhive 是基于 **Git Worktrees + tmux** 构建的并行开发工作流管理器。一条命令即可创建完整的 AI 编程环境。

### 从零开始（5 分钟）

```bash
# 1. 安装
git clone https://github.com/yboker/workhive.git ~/workhive
export PATH="$HOME/workhive/bin:$PATH"

# 2. 初始化项目
cd /path/to/your/project
workhive init

# 3. 进入工作空间
workhive open feature-login

# 4. 日常操作
workhive open feature-payment   # 新建分支工作空间
workhive ls                     # 列出所有工作空间
workhive list                   # 状态仪表盘
workhive attach feature-login  # 在 tmux 内切换
workhive gc --auto              # 清理已合并分支
```

### 命令速查

| 命令 | 别名 | 说明 |
|------|------|------|
| `workhive init` | | 初始化 workhive（交互式向导） |
| `workhive open [branch]` | | 创建/进入工作空间 |
| `workhive attach [branch]` | | 连接到已有会话 |
| `workhive list` | `ls` | 列出所有工作空间（含安全标记） |
| `workhive list` | `ls` | 工作空间状态仪表盘 |
| `workhive remove <branch>` | `rm` | 删除工作空间（含安全警告） |
| `workhive rename <old> <new>` | | 重命名分支+工作空间+状态 |
| `workhive destroy [--global]` | | 删除项目所有工作空间（含安全扫描） |
| `workhive gc [--auto]` | | 清理已合并分支和孤立会话 |
| `workhive config` | | 查看/编辑当前配置 |
| `workhive doctor` | | 依赖和配置健康检查 |
| `workhive log` | | 查看操作日志 |
| `workhive pane <action>` | | 管理 tmux 面板 |
| `workhive upgrade [--check]` | | 自动升级到最新版 |
| `workhive shell setup [--dry-run]` | | 配置 Shell 集成与补全 |

### 安全特性

- **Worktree 安全扫描** — 销毁/删除前自动检测未提交更改和未合并提交
- **安全删除保护** — 拒绝删除 `/`、`$HOME` 等危险路径
- **插件权限检查** — 拒绝加载不安全的插件文件
- **配置注入防护** — 验证配置键名格式，防止 yq 命令注入

### 依赖要求

| 依赖 | 最低版本 | 安装方式 |
|------|----------|----------|
| Git | 2.20+ | `brew install git` |
| tmux | 3.2+ | `brew install tmux` |
| yq | 4.x | `brew install yq` |
| Bash | 4.3+ | 通常已预装 |

### 主题

8 个内置 tmux 主题：`catppuccin-mocha`（默认）、`tokyo-night`、`dracula`、`nord`、`gruvbox`、`rose-pine`、`everforest`、`onedark`

通过 `workhive init` 或 `workhive config` 选择主题，支持实时预览。
