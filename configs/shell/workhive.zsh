# workhive Zsh Integration - DO NOT EDIT MANUALLY
# This file is managed by workhive. Any changes may be overwritten.

[[ -n "${_WORKHIVE_ZSH_LOADED:-}" ]] && return 0
_WORKHIVE_ZSH_LOADED=1

# ── Smart History Configuration ──────────────────
HISTSIZE="${HISTSIZE:-50000}"
SAVEHIST="${SAVEHIST:-50000}"
if [[ -z "${HISTFILE:-}" ]]; then
    HISTFILE="${ZDOTDIR:-$HOME}/.zsh_history"
fi
setopt HIST_FIND_NO_DUPS
setopt SHARE_HISTORY
setopt APPEND_HISTORY
setopt INC_APPEND_HISTORY
setopt EXTENDED_HISTORY

# ── Color output ─────────────────────────────────
export CLICOLOR=1
export LSCOLORS="Gxfxcxdxbxegedabagacad"

# ── Default Zsh options ──────────────────────────
setopt interactive_comments
bindkey -e

# ── Directory Navigation ────────────────────────
setopt auto_cd
setopt auto_pushd
setopt pushd_ignore_dups
setopt pushdminus

# ── Common Aliases ───────────────────────────────
alias ll='ls -lhF'
alias la='ls -lAhF'
alias l='ls -CF'
alias ...='../..'
alias ....='../../..'
alias .....='../../../..'
alias md='mkdir -p'
alias rd=rmdir
alias grep='grep --color=auto'

# ── Git Aliases ──────────────────────────────────
alias g='git'
alias ga='git add'
alias gaa='git add --all'
alias gb='git branch'
alias gc='git commit -v'
alias gcmsg='git commit -m'
alias gco='git checkout'
alias gcb='git checkout -b'
alias gd='git diff'
alias gds='git diff --staged'
alias gf='git fetch'
alias gl='git pull'
alias gp='git push'
alias gst='git status'
alias gss='git status -s'
alias glo='git log --oneline --decorate'
alias glg='git log --stat'

# ── Zoxide integration (smart cd) ────────────────
if command -v zoxide &>/dev/null; then
    eval "$(zoxide init zsh)"
fi

# ── Starship prompt (only if not already loaded) ─
if [[ -z "${STARSHIP_SHELL:-}" ]] && command -v starship &>/dev/null; then
    eval "$(starship init zsh)"
fi

# ── SSH TERM fix ─────────────────────────────────
if [[ "$TERM" == "tmux-256color" ]]; then
    ssh() { TERM=xterm-256color command ssh "$@"; }
fi
