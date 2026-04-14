# workhive - Fish Tab Completion
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
#
# Installation (choose one):
#   1. Copy to fish completions:
#      cp completions/workhive.fish ~/.config/fish/completions/workhive.fish
#
#   2. Managed setup (recommended):
#      workhive shell setup --shell fish --profile minimal --enable completions

function __workhive_get_worktrees
    set -l worktree_base ".workhive/worktrees"
    if test -d "$worktree_base"
        for d in $worktree_base/*/
            if test -d "$d"
                basename "$d"
            end
        end
    end
end

function __workhive_get_sessions
    set -l tag ""
    if command -q yq; and test -f ".workhive/config.yaml"
        set tag (yq -r '.tmux.session_prefix // ""' .workhive/config.yaml 2>/dev/null)
        test "$tag" = "null"; and set tag ""
    end
    if test -z "$tag"
        set tag "wh"
    end
    set -l project_name (cd (git rev-parse --show-toplevel 2>/dev/null; or pwd) && basename (pwd))
    if command -q tmux
        for s in (tmux list-sessions -F '#{session_name}' 2>/dev/null)
            if string match -q "$tag@*@$project_name@*" "$s"
                # Extract branch (last @ segment)
                set -l branch (string replace -r '.*@' '' "$s")
                echo "$branch"
            end
        end
    end
end

function __workhive_needs_command
    set -l cmd (commandline -opc)
    test (count $cmd) -eq 1
end

function __workhive_using_command
    set -l cmd (commandline -opc)
    test (count $cmd) -ge 2; and test "$cmd[2]" = "$argv[1]"
end

function __workhive_using_pane_subcommand
    set -l cmd (commandline -opc)
    test (count $cmd) -ge 3; and test "$cmd[2]" = "pane"; and test "$cmd[3]" = "$argv[1]"
end

# Disable file completions by default
complete -c workhive -f

# Global options
complete -c workhive -n __workhive_needs_command -s v -l version -d "Show version"
complete -c workhive -n __workhive_needs_command -s h -l help -d "Show help"
complete -c workhive -n __workhive_needs_command -l debug -d "Enable debug mode"

# Commands
complete -c workhive -n __workhive_needs_command -a init -d "Interactive project configuration"
complete -c workhive -n __workhive_needs_command -a open -d "Create or enter a Worktree workspace"
complete -c workhive -n __workhive_needs_command -a attach -d "Re-enter an existing workspace"
complete -c workhive -n __workhive_needs_command -a close -d "Close current workspace"
complete -c workhive -n __workhive_needs_command -a list -d "Global workspace dashboard"
complete -c workhive -n __workhive_needs_command -a ls -d "Global workspace dashboard"
complete -c workhive -n __workhive_needs_command -a remove -d "Remove a Worktree"
complete -c workhive -n __workhive_needs_command -a rm -d "Remove a Worktree"
complete -c workhive -n __workhive_needs_command -a pane -d "Manage tmux panes"
complete -c workhive -n __workhive_needs_command -a config -d "View or edit configuration"
complete -c workhive -n __workhive_needs_command -a tools -d "Manage global tools registry"
complete -c workhive -n __workhive_needs_command -a doctor -d "Check environment health"
complete -c workhive -n __workhive_needs_command -a rename -d "Rename a worktree branch"
complete -c workhive -n __workhive_needs_command -a log -d "View workhive logs"
complete -c workhive -n __workhive_needs_command -a gc -d "Clean up merged branches"
complete -c workhive -n __workhive_needs_command -a destroy -d "Destroy all sessions and worktrees"
complete -c workhive -n __workhive_needs_command -a upgrade -d "Check for updates and upgrade"
complete -c workhive -n __workhive_needs_command -a shell -d "Manage shell integration"

# init
complete -c workhive -n "__workhive_using_command init" -l non-interactive -d "Use default config"
complete -c workhive -n "__workhive_using_command init" -s f -l force -d "Force re-init"
complete -c workhive -n "__workhive_using_command init" -l global -d "Initialize global config"
complete -c workhive -n "__workhive_using_command init" -l debug -d "Enable debug mode"
complete -c workhive -n "__workhive_using_command init" -s h -l help -d "Show help"

# open
complete -c workhive -n "__workhive_using_command open" -a "(__workhive_get_worktrees; __workhive_get_sessions)" -d "Worktree"
complete -c workhive -n "__workhive_using_command open" -s f -l force -d "Force recreate session"
complete -c workhive -n "__workhive_using_command open" -l force-layout -d "Rebuild layout from config"
complete -c workhive -n "__workhive_using_command open" -l no-attach -d "Do not auto-attach"
complete -c workhive -n "__workhive_using_command open" -s h -l help -d "Show help"

# attach
complete -c workhive -n "__workhive_using_command attach" -a "(__workhive_get_sessions)" -d "Session"
complete -c workhive -n "__workhive_using_command attach" -s r -l read-only -d "Read-only mode"
complete -c workhive -n "__workhive_using_command attach" -l force-layout -d "Rebuild layout from config"
complete -c workhive -n "__workhive_using_command attach" -s h -l help -d "Show help"

# pane subcommands
complete -c workhive -n "__workhive_using_command pane" -a open -d "Open tool in current pane"
complete -c workhive -n "__workhive_using_command pane" -a close -d "Close current pane"
complete -c workhive -n "__workhive_using_command pane" -a split -d "Split current pane below"
complete -c workhive -n "__workhive_using_command pane" -a send -d "Send text to a pane"
complete -c workhive -n "__workhive_using_command pane" -a list -d "List panes in current session"
complete -c workhive -n "__workhive_using_command pane" -a current -d "Show current pane details"
complete -c workhive -n "__workhive_using_command pane" -s h -l help -d "Show help"

# pane open
complete -c workhive -n "__workhive_using_pane_subcommand open" -a "(__workhive_get_worktrees; __workhive_get_sessions)" -d "Worktree"
complete -c workhive -n "__workhive_using_pane_subcommand open" -l cmd -d "Command to run (skip interactive)" -a "claude codex opencode lazygit yazi shell" -r
complete -c workhive -n "__workhive_using_pane_subcommand open" -s f -l force -d "Force create worktree"
complete -c workhive -n "__workhive_using_pane_subcommand open" -s h -l help -d "Show help"

# pane close
complete -c workhive -n "__workhive_using_pane_subcommand close" -l id -d "Close a pane by tmux pane id" -r
complete -c workhive -n "__workhive_using_pane_subcommand close" -s f -l force -d "Skip confirmation"
complete -c workhive -n "__workhive_using_pane_subcommand close" -s h -l help -d "Show help"

# pane split
complete -c workhive -n "__workhive_using_pane_subcommand split" -l cmd -d "Command to run in new pane"
complete -c workhive -n "__workhive_using_pane_subcommand split" -l size -d "Split percentage"
complete -c workhive -n "__workhive_using_pane_subcommand split" -s h -l help -d "Show help"

# pane send
complete -c workhive -n "__workhive_using_pane_subcommand send" -l all -d "Broadcast text to all panes in current session"
complete -c workhive -n "__workhive_using_pane_subcommand send" -l mode -d "Injection mode" -a "auto plain paste bracketed-paste"
complete -c workhive -n "__workhive_using_pane_subcommand send" -l commit -d "Commit key mode" -a "auto enter c-m none"
complete -c workhive -n "__workhive_using_pane_subcommand send" -l verbose -d "Print resolved strategy per target pane"
complete -c workhive -n "__workhive_using_pane_subcommand send" -l role -d "Resolve pane by layout role" -r
complete -c workhive -n "__workhive_using_pane_subcommand send" -l diagnose -d "Diagnose pane send path only (read-only)"
complete -c workhive -n "__workhive_using_pane_subcommand send" -s h -l help -d "Show help"

# pane list
complete -c workhive -n "__workhive_using_pane_subcommand list" -l format -d "Output format" -a "table json"
complete -c workhive -n "__workhive_using_pane_subcommand list" -l deep -d "Enable deep runtime probe (slower, richer diagnostics)"
complete -c workhive -n "__workhive_using_pane_subcommand list" -s h -l help -d "Show help"

# pane current
complete -c workhive -n "__workhive_using_pane_subcommand current" -l format -d "Output format" -a "table json"
complete -c workhive -n "__workhive_using_pane_subcommand current" -l deep -d "Enable deep runtime probe (slower, richer diagnostics)"
complete -c workhive -n "__workhive_using_pane_subcommand current" -s h -l help -d "Show help"

# remove/rm
complete -c workhive -n "__workhive_using_command remove; or __workhive_using_command rm" -a "(__workhive_get_worktrees; __workhive_get_sessions)" -d "Worktree"
complete -c workhive -n "__workhive_using_command remove; or __workhive_using_command rm" -s f -l force -d "Force remove"
complete -c workhive -n "__workhive_using_command remove; or __workhive_using_command rm" -s h -l help -d "Show help"

# config
complete -c workhive -n "__workhive_using_command config" -l edit -d "Open config in editor"
complete -c workhive -n "__workhive_using_command config" -l global -d "View/modify global config"
complete -c workhive -n "__workhive_using_command config" -l layout -d "Open interactive layout editor (config only)"
complete -c workhive -n "__workhive_using_command config" -s h -l help -d "Show help"

# tools
complete -c workhive -n "__workhive_using_command tools" -a "list add edit remove rm set-default validate manage" -d "Subcommand"
complete -c workhive -n "__workhive_using_command tools" -l name -d "Tool name" -r
complete -c workhive -n "__workhive_using_command tools" -l label -d "Tool label" -r
complete -c workhive -n "__workhive_using_command tools" -l cmd -d "Tool command" -r
complete -c workhive -n "__workhive_using_command tools" -l command -d "Tool command" -r
complete -c workhive -n "__workhive_using_command tools" -l ai -d "AI tool true/false" -a "true false" -r
complete -c workhive -n "__workhive_using_command tools" -l enable -d "Enabled true/false" -a "true false" -r
complete -c workhive -n "__workhive_using_command tools" -l default -d "Default true/false" -a "true false" -r
complete -c workhive -n "__workhive_using_command tools" -s h -l help -d "Show help"

# list/ls
complete -c workhive -n "__workhive_using_command list; or __workhive_using_command ls" -l format -d "Output format" -a "table json"
complete -c workhive -n "__workhive_using_command list; or __workhive_using_command ls" -l layout -d "Show detailed layout configuration"
complete -c workhive -n "__workhive_using_command list; or __workhive_using_command ls" -l watch -d "Watch mode: auto refresh dashboard"
complete -c workhive -n "__workhive_using_command list; or __workhive_using_command ls" -l interval -d "Watch refresh interval (seconds)" -r
complete -c workhive -n "__workhive_using_command list; or __workhive_using_command ls" -s h -l help -d "Show help"

# close
complete -c workhive -n "__workhive_using_command close" -s f -l force -d "Skip confirmation"
complete -c workhive -n "__workhive_using_command close" -s h -l help -d "Show help"

# doctor
complete -c workhive -n "__workhive_using_command doctor" -l format -d "Output format" -a "table json"
complete -c workhive -n "__workhive_using_command doctor" -s h -l help -d "Show help"

# rename
complete -c workhive -n "__workhive_using_command rename" -a "(__workhive_get_worktrees; __workhive_get_sessions)" -d "Branch"
complete -c workhive -n "__workhive_using_command rename" -s f -l force -d "Force rename"
complete -c workhive -n "__workhive_using_command rename" -s h -l help -d "Show help"

# log
complete -c workhive -n "__workhive_using_command log" -l level -d "Log level" -a "debug info warn error" -r
complete -c workhive -n "__workhive_using_command log" -s f -l follow -d "Follow log output"
complete -c workhive -n "__workhive_using_command log" -s n -l lines -d "Number of lines" -r
complete -c workhive -n "__workhive_using_command log" -s h -l help -d "Show help"

# gc
complete -c workhive -n "__workhive_using_command gc" -l auto -d "Auto clean without confirmation"
complete -c workhive -n "__workhive_using_command gc" -l snapshots -d "Clean snapshot files only"
complete -c workhive -n "__workhive_using_command gc" -s h -l help -d "Show help"

# destroy
complete -c workhive -n "__workhive_using_command destroy" -s f -l force -d "Force destroy"
complete -c workhive -n "__workhive_using_command destroy" -s h -l help -d "Show help"

# upgrade
complete -c workhive -n "__workhive_using_command upgrade" -l check -d "Check for updates only"
complete -c workhive -n "__workhive_using_command upgrade" -l version -d "Install specific version"
complete -c workhive -n "__workhive_using_command upgrade" -s f -l force -d "Force reinstall"
complete -c workhive -n "__workhive_using_command upgrade" -s h -l help -d "Show help"
