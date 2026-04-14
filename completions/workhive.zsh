#compdef workhive
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# workhive - Zsh Tab Completion (native)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
#
# Installation (choose one):
#   1. Copy to fpath directory:
#      cp completions/workhive.zsh ~/.zsh/completions/_workhive
#      Then add to ~/.zshrc: fpath=(~/.zsh/completions $fpath)
#
#   2. Source directly:
#      source /path/to/workhive/completions/workhive.zsh

_workhive_worktrees() {
    local worktree_base=".workhive/worktrees"
    local -a worktrees
    if [[ -d "$worktree_base" ]]; then
        worktrees=(${worktree_base}/*(/:t))
    fi
    _describe 'worktree' worktrees
}

_workhive_sessions() {
    local tag=""
    if command -v yq &>/dev/null && [[ -f ".workhive/config.yaml" ]]; then
        tag=$(yq -r '.tmux.session_prefix // ""' .workhive/config.yaml 2>/dev/null)
        [[ "$tag" == "null" ]] && tag=""
    fi
    [[ -z "$tag" ]] && tag="wh"
    local project_name
    project_name=$(cd "$(git rev-parse --show-toplevel 2>/dev/null || pwd)" && basename "$(pwd)")
    local -a sessions
    if command -v tmux &>/dev/null; then
        local raw_sessions
        raw_sessions=(${(f)"$(tmux list-sessions -F '#{session_name}' 2>/dev/null)"})
        for s in "${raw_sessions[@]}"; do
            if [[ "$s" == "${tag}@"*"@${project_name}@"* ]]; then
                sessions+=("${s##*@}")
            fi
        done
    fi
    _describe 'session' sessions
}

_workhive_worktrees_and_sessions() {
    _workhive_worktrees
    _workhive_sessions
}

_workhive() {
    local -a commands
    commands=(
        'init:Interactive project configuration'
        'open:Create or enter a Worktree workspace'
        'attach:Re-enter an existing workspace'
        'close:Close current workspace and return to terminal'
        'list:Global workspace dashboard'
        'ls:Global workspace dashboard'
        'remove:Remove a Worktree'
        'rm:Remove a Worktree'
        'pane:Manage tmux panes'
        'config:View or edit configuration'
        'tools:Manage global tools registry'
        'doctor:Check environment health'
        'rename:Rename a worktree branch'
        'log:View workhive logs'
        'gc:Clean up merged branches'
        'destroy:Destroy all sessions and worktrees'
        'upgrade:Check for updates and upgrade'
        'shell:Manage shell integration'
    )

    _arguments -C \
        '(-v --version)'{-v,--version}'[Show version]' \
        '(-h --help)'{-h,--help}'[Show help]' \
        '--debug[Enable debug mode]' \
        '1:command:->command' \
        '*::arg:->args'

    case "$state" in
        command)
            _describe 'command' commands
            ;;
        args)
            case "${words[1]}" in
                open)
                    _arguments \
                        '1:worktree:_workhive_worktrees_and_sessions' \
                        '(-f --force)'{-f,--force}'[Force recreate session]' \
                        '--force-layout[Rebuild layout from config]' \
                        '--no-attach[Do not auto-attach]' \
                        '(-h --help)'{-h,--help}'[Show help]'
                    ;;
                attach)
                    _arguments \
                        '1:session:_workhive_sessions' \
                        '(-r --read-only)'{-r,--read-only}'[Read-only mode]' \
                        '--force-layout[Rebuild layout from config]' \
                        '(-h --help)'{-h,--help}'[Show help]'
                    ;;
                pane)
                    local -a pane_commands
                    pane_commands=(
                        'open:Open a tool in current pane for a Worktree'
                        'close:Close current pane'
                        'split:Split current pane below'
                        'send:Send text to a specific pane'
                        'list:List all panes in current session'
                        'current:Show current pane details'
                    )
                    _arguments -C \
                        '(-h --help)'{-h,--help}'[Show help]' \
                        '1:subcommand:->pane_cmd' \
                        '*::pane_arg:->pane_args'

                    case "$state" in
                        pane_cmd)
                            _describe 'pane command' pane_commands
                            ;;
                        pane_args)
                            case "${words[1]}" in
                                open)
                                    _arguments \
                                        '1:worktree:_workhive_worktrees_and_sessions' \
                                        '--cmd[Command to run (skip interactive menu)]:command:(claude codex opencode lazygit yazi shell)' \
                                        '(-f --force)'{-f,--force}'[Force create worktree if not exists]' \
                                        '(-h --help)'{-h,--help}'[Show help]'
                                    ;;
                                split)
                                    _arguments \
                                        '--cmd[Command to run in new pane]:command' \
                                        '--size[Split percentage]:percentage' \
                                        '(-h --help)'{-h,--help}'[Show help]'
                                    ;;
                                close)
                                    _arguments \
                                        '--id[Close a pane by tmux pane id]:pane id:' \
                                        '(-f --force)'{-f,--force}'[Skip confirmation]' \
                                        '(-h --help)'{-h,--help}'[Show help]'
                                    ;;
                                send)
                                    _arguments \
                                        '1:pane index:' \
                                        '2:text:' \
                                        '--all[Broadcast text to all panes in current session]' \
                                        '--mode[Injection mode]:mode:(auto plain paste bracketed-paste)' \
                                        '--commit[Commit key mode]:mode:(auto enter c-m none)' \
                                        '--verbose[Print resolved strategy per target pane]' \
                                        '--role[Resolve pane by role]:role:' \
                                        '--diagnose[Diagnose pane send path only (read-only)]' \
                                        '(-h --help)'{-h,--help}'[Show help]'
                                    ;;
                                list|ls)
                                    _arguments \
                                        '--format[Output format]:format:(table json)' \
                                        '--deep[Enable deep runtime probe (slower, richer diagnostics)]' \
                                        '(-h --help)'{-h,--help}'[Show help]'
                                    ;;
                                current|cur)
                                    _arguments \
                                        '--format[Output format]:format:(table json)' \
                                        '--deep[Enable deep runtime probe (slower, richer diagnostics)]' \
                                        '(-h --help)'{-h,--help}'[Show help]'
                                    ;;
                            esac
                            ;;
                    esac
                    ;;
                remove|rm)
                    _arguments \
                        '1:worktree:_workhive_worktrees_and_sessions' \
                        '(-f --force)'{-f,--force}'[Force remove]' \
                        '(-h --help)'{-h,--help}'[Show help]'
                    ;;
                init)
                    _arguments \
                        '--non-interactive[Use default config]' \
                        '(-f --force)'{-f,--force}'[Force re-init]' \
                        '--global[Initialize global config]' \
                        '--debug[Enable debug mode]' \
                        '(-h --help)'{-h,--help}'[Show help]'
                    ;;
                config)
                    _arguments \
                        '--edit[Open config in editor]' \
                        '--global[View/modify global config]' \
                        '--layout[Open interactive layout editor (config only)]' \
                        '(-h --help)'{-h,--help}'[Show help]'
                    ;;
                tools)
                    _arguments \
                        '1:subcommand:(list add edit remove rm set-default validate manage)' \
                        '--name[Tool name]:name:' \
                        '--label[Tool label]:label:' \
                        '--cmd[Tool command]:command:' \
                        '--ai[Whether AI tool]:bool:(true false)' \
                        '--enable[Whether enabled]:bool:(true false)' \
                        '--default[Whether default]:bool:(true false)' \
                        '(-h --help)'{-h,--help}'[Show help]'
                    ;;
                list|ls)
                    _arguments \
                        '--format[Output format]:format:(table json)' \
                        '--layout[Show detailed layout configuration]' \
                        '--watch[Watch mode: auto refresh dashboard]' \
                        '--interval[Watch refresh interval (seconds)]:seconds:' \
                        '(-h --help)'{-h,--help}'[Show help]'
                    ;;
                close)
                    _arguments \
                        '(-f --force)'{-f,--force}'[Skip confirmation]' \
                        '(-h --help)'{-h,--help}'[Show help]'
                    ;;
                doctor)
                    _arguments \
                        '--format[Output format]:format:(table json)' \
                        '(-h --help)'{-h,--help}'[Show help]'
                    ;;
                rename)
                    _arguments \
                        '1:old branch:_workhive_worktrees_and_sessions' \
                        '2:new branch:' \
                        '(-f --force)'{-f,--force}'[Force rename]' \
                        '(-h --help)'{-h,--help}'[Show help]'
                    ;;
                log)
                    _arguments \
                        '--level[Log level]:level:(debug info warn error)' \
                        '(-f --follow)'{-f,--follow}'[Follow log output]' \
                        '(-n --lines)'{-n,--lines}'[Number of lines]:count:' \
                        '(-h --help)'{-h,--help}'[Show help]'
                    ;;
                gc)
                    _arguments \
                        '--auto[Auto clean without confirmation]' \
                        '--snapshots[Clean snapshot files only]' \
                        '(-h --help)'{-h,--help}'[Show help]'
                    ;;
                destroy)
                    _arguments \
                        '(-f --force)'{-f,--force}'[Force destroy]' \
                        '(-h --help)'{-h,--help}'[Show help]'
                    ;;
                upgrade)
                    _arguments \
                        '--check[Check for updates only]' \
                        '--version[Install specific version]:version:' \
                        '(-f --force)'{-f,--force}'[Force reinstall]' \
                        '(-h --help)'{-h,--help}'[Show help]'
                    ;;
                shell)
                    _arguments -C \
                        '(-h --help)'{-h,--help}'[Show help]' \
                        '1:subcommand:(init remove show)' \
                        '--shell[Target shell]:shell:(zsh bash)' \
                        '--dry-run[Preview changes]'
                    ;;
            esac
            ;;
    esac
}

# When sourced via eval, register with compdef; when loaded via fpath, #compdef handles it
if [[ -z "${_comps[workhive]:-}" ]]; then
    compdef _workhive workhive 2>/dev/null
fi
