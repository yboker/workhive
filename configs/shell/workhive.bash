# workhive Bash Integration - DO NOT EDIT MANUALLY
# This file is managed by workhive. Any changes may be overwritten.

[[ -n "${_WORKHIVE_BASH_LOADED:-}" ]] && return 0
_WORKHIVE_BASH_LOADED=1

# ── Smart History Configuration ──────────────────
HISTSIZE="${HISTSIZE:-50000}"
HISTFILESIZE="${HISTFILESIZE:-50000}"
HISTCONTROL=ignoredups:erasedups
shopt -s histappend

# ── Color output ─────────────────────────────────
export CLICOLOR=1

# ── Common Aliases ───────────────────────────────
alias ll='ls -lhF'
alias la='ls -lAhF'
alias l='ls -CF'
alias ...='cd ../..'
alias ....='cd ../../..'
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
    eval "$(zoxide init bash)"
fi

# ── Starship prompt (only if not already loaded) ─
if [[ -z "${STARSHIP_SHELL:-}" ]] && command -v starship &>/dev/null; then
    eval "$(starship init bash)"
fi
