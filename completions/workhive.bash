#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# workhive - Bash Tab Completion
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
#
# Installation:
#   source /path/to/workhive/completions/workhive.bash
#
# Or add to ~/.bashrc / ~/.zshrc:
#   [[ -f /path/to/workhive/completions/workhive.bash ]] && \
#     source /path/to/workhive/completions/workhive.bash

_workhive_get_worktrees() {
    local worktree_base=".workhive/worktrees"
    if [[ -d "$worktree_base" ]]; then
        for d in "$worktree_base"/*/; do
            [[ -d "$d" ]] && basename "$d"
        done
    fi
}

_workhive_get_sessions() {
    local tag=""
    if command -v yq &>/dev/null && [[ -f ".workhive/config.yaml" ]]; then
        tag=$(yq -r '.tmux.session_prefix // ""' .workhive/config.yaml 2>/dev/null)
        [[ "$tag" == "null" ]] && tag=""
    fi
    [[ -z "$tag" ]] && tag="wh"
    local project_name
    project_name=$(cd "$(git rev-parse --show-toplevel 2>/dev/null || pwd)" && basename "$(pwd)")
    if command -v tmux &>/dev/null; then
        tmux list-sessions -F '#{session_name}' 2>/dev/null | while IFS= read -r s; do
            if [[ "$s" == "${tag}@"*"@${project_name}@"* ]]; then
                echo "${s##*@}"
            fi
        done
    fi
}

_workhive_completions() {
    local cur prev words cword
    if declare -F _init_completion &>/dev/null; then
        _init_completion || return
    else
        cur="${COMP_WORDS[COMP_CWORD]}"
        prev="${COMP_WORDS[COMP_CWORD-1]}"
        words=("${COMP_WORDS[@]}")
        cword=$COMP_CWORD
    fi

    local commands="init open attach close list ls remove rm pane config tools doctor rename log gc destroy upgrade shell"
    local pane_subcommands="open close split send list current"
    local global_opts="--version -v --help -h --debug"

    case "$cword" in
        1)
            COMPREPLY=($(compgen -W "$commands $global_opts" -- "$cur"))
            return
            ;;
    esac

    local cmd="${words[1]}"

    case "$cmd" in
        open)
            case "$cword" in
                2)
                    local worktrees
                    worktrees=$(_workhive_get_worktrees)
                    local sessions
                    sessions=$(_workhive_get_sessions)
                    COMPREPLY=($(compgen -W "$worktrees $sessions --force -f --force-layout --no-attach --help -h" -- "$cur"))
                    ;;
                *)
                    COMPREPLY=($(compgen -W "--force -f --force-layout --no-attach --help -h" -- "$cur"))
                    ;;
            esac
            ;;
        attach)
            case "$cword" in
                2)
                    local sessions
                    sessions=$(_workhive_get_sessions)
                    COMPREPLY=($(compgen -W "$sessions --read-only -r --force-layout --help -h" -- "$cur"))
                    ;;
                *)
                    COMPREPLY=($(compgen -W "--read-only -r --help -h" -- "$cur"))
                    ;;
            esac
            ;;
        pane)
            case "$cword" in
                2)
                    COMPREPLY=($(compgen -W "$pane_subcommands --help -h" -- "$cur"))
                    ;;
                3)
                    local subcmd="${words[2]}"
                    case "$subcmd" in
                        open)
                            local worktrees
                            worktrees=$(_workhive_get_worktrees)
                            local sessions
                            sessions=$(_workhive_get_sessions)
                            COMPREPLY=($(compgen -W "$worktrees $sessions --cmd --force -f --help -h" -- "$cur"))
                            ;;
                        split)
                            COMPREPLY=($(compgen -W "--cmd --size --help -h" -- "$cur"))
                            ;;
                        close)
                            COMPREPLY=($(compgen -W "--id --force -f --help -h" -- "$cur"))
                            ;;
                        send)
                            COMPREPLY=($(compgen -W "--all --mode --commit --verbose --role --diagnose --help -h" -- "$cur"))
                            ;;
                        list|ls)
                            COMPREPLY=($(compgen -W "--format --deep --help -h" -- "$cur"))
                            ;;
                        current|cur)
                            COMPREPLY=($(compgen -W "--format --deep --help -h" -- "$cur"))
                            ;;
                    esac
                    ;;
                *)
                    local subcmd="${words[2]}"
                    case "$subcmd" in
                        open)
                            COMPREPLY=($(compgen -W "--cmd --force -f --help -h" -- "$cur"))
                            ;;
                        split)
                            COMPREPLY=($(compgen -W "--cmd --size --help -h" -- "$cur"))
                            ;;
                        close)
                            COMPREPLY=($(compgen -W "--id --force -f --help -h" -- "$cur"))
                            ;;
                        list|ls)
                            COMPREPLY=($(compgen -W "--format --deep --help -h" -- "$cur"))
                            ;;
                        current|cur)
                            COMPREPLY=($(compgen -W "--format --deep --help -h" -- "$cur"))
                            ;;
                    esac
                    ;;
            esac
            ;;
        remove|rm)
            case "$cword" in
                2)
                    local worktrees
                    worktrees=$(_workhive_get_worktrees)
                    local sessions
                    sessions=$(_workhive_get_sessions)
                    COMPREPLY=($(compgen -W "$worktrees $sessions --force -f --help -h" -- "$cur"))
                    ;;
                *)
                    COMPREPLY=($(compgen -W "--force -f --help -h" -- "$cur"))
                    ;;
            esac
            ;;
        init)
            COMPREPLY=($(compgen -W "--non-interactive --force -f --global --debug --help -h" -- "$cur"))
            ;;
        config)
            COMPREPLY=($(compgen -W "--edit --global --layout --help -h" -- "$cur"))
            ;;
        tools)
            case "$cword" in
                2)
                    COMPREPLY=($(compgen -W "list add edit remove rm set-default validate manage --help -h" -- "$cur"))
                    ;;
                *)
                    COMPREPLY=($(compgen -W "--name --label --cmd --ai --enable --default --help -h" -- "$cur"))
                    ;;
            esac
            ;;
        list|ls)
            COMPREPLY=($(compgen -W "--format --layout --watch --interval --help -h" -- "$cur"))
            ;;
        close)
            COMPREPLY=($(compgen -W "--force -f --help -h" -- "$cur"))
            ;;
        doctor)
            COMPREPLY=($(compgen -W "--format --help -h" -- "$cur"))
            ;;
        rename)
            case "$cword" in
                2)
                    local worktrees
                    worktrees=$(_workhive_get_worktrees)
                    local sessions
                    sessions=$(_workhive_get_sessions)
                    COMPREPLY=($(compgen -W "$worktrees $sessions --force -f --help -h" -- "$cur"))
                    ;;
                *)
                    COMPREPLY=($(compgen -W "--force -f --help -h" -- "$cur"))
                    ;;
            esac
            ;;
        log)
            COMPREPLY=($(compgen -W "--level --follow -f --lines -n --help -h" -- "$cur"))
            ;;
        gc)
            COMPREPLY=($(compgen -W "--auto --snapshots --help -h" -- "$cur"))
            ;;
        destroy)
            COMPREPLY=($(compgen -W "--force -f --help -h" -- "$cur"))
            ;;
        upgrade)
            COMPREPLY=($(compgen -W "--check --version --force -f --help -h" -- "$cur"))
            ;;
    esac
}

complete -F _workhive_completions workhive

# Zsh compatibility: if running in zsh, enable bash completion compatibility
if [[ -n "${ZSH_VERSION:-}" ]]; then
    autoload -Uz bashcompinit 2>/dev/null && bashcompinit
fi
