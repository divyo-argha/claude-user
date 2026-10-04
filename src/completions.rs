use anyhow::{bail, Result};

pub fn print_completions(shell: &str) -> Result<()> {
    match shell.to_lowercase().as_str() {
        "zsh" => {
            print!("{ZSH_COMPLETIONS}");
            Ok(())
        }
        "bash" => {
            print!("{BASH_COMPLETIONS}");
            Ok(())
        }
        "fish" => {
            print!("{FISH_COMPLETIONS}");
            Ok(())
        }
        "powershell" | "pwsh" => {
            print!("{PWSH_COMPLETIONS}");
            Ok(())
        }
        other => bail!(
            "unsupported shell: '{other}'. Supported shells: bash, zsh, fish, powershell\nExample usage:\n  eval \"$(cuser completions zsh)\"\n  eval \"$(cuser completions bash)\""
        ),
    }
}

const ZSH_COMPLETIONS: &str = r#"#compdef cuser claude-user

_cuser() {
    local -a commands
    commands=(
        'switch:Switch active profile without launching'
        'run:Launch isolated session mode'
        'list:List existing profiles with quota'
        'usage:Show detailed quota and rate limit breakdown'
        'auto:Auto-rotate accounts before hitting rate limits'
        'alias:Assign or view short profile aliases'
        'unalias:Remove a profile alias'
        'config:View and edit tool settings'
        'add-token:Register profile from OAuth setup token or API key'
        'export:Export profiles and credentials to a backup JSON'
        'import:Import backup file or ~/.claude'
        'import-usage:Import usage snapshot readings from another machine'
        'current:Show the active profile'
        'status:Show the active profile'
        'map:Bind a directory to a profile'
        'unmap:Remove a directory mapping'
        'disable:Hold a profile out of rotation'
        'enable:Re-enable a disabled profile'
        'sync:Copy shared config into every existing profile'
        'remove:Delete a profile'
        'rename:Rename a profile'
        'purge:Remove all claude-user data'
        'completions:Generate shell autocompletions'
    )

    local -a profiles
    profiles=($(_cuser_profiles 2>/dev/null))

    _arguments \
        '1: :->command' \
        '*: :->args'

    case $state in
        command)
            _describe -t commands 'commands' commands
            _describe -t profiles 'profiles' profiles
            ;;
        args)
            case $words[1] in
                switch|run|usage|quota|disable|enable|remove|rm|delete|map)
                    _describe -t profiles 'profiles' profiles
                    ;;
                alias)
                    _describe -t profiles 'profiles' profiles
                    ;;
                completions)
                    _values 'shell' bash zsh fish powershell
                    ;;
                config)
                    _values 'action' get set unset path list
                    ;;
                *)
                    ;;
            esac
            ;;
    esac
}

_cuser_profiles() {
    local root="${CLAUDE_PROFILES_DIR:-$HOME/.claude-profiles}"
    if [ -d "$root" ]; then
        for d in "$root"/*; do
            if [ -d "$d" ]; then
                local base=$(basename "$d")
                if [ "$base" != "shared" ]; then
                    echo "$base"
                fi
            fi
        done
    fi
}

compdef _cuser cuser claude-user
"#;

const BASH_COMPLETIONS: &str = r#"_cuser() {
    local cur prev words cword
    _init_completion 2>/dev/null || {
        cur="${COMP_WORDS[COMP_CWORD]}"
        prev="${COMP_WORDS[COMP_CWORD-1]}"
        words=("${COMP_WORDS[@]}")
        cword=$COMP_CWORD
    }

    local commands="switch run list usage auto alias unalias config add-token export import import-usage current status map unmap disable enable sync remove rename purge completions --help --version --update --json --refresh --token-status"
    
    local profiles=""
    local root="${CLAUDE_PROFILES_DIR:-$HOME/.claude-profiles}"
    if [ -d "$root" ]; then
        for d in "$root"/*; do
            if [ -d "$d" ]; then
                local b=$(basename "$d")
                if [ "$b" != "shared" ]; then
                    profiles="$profiles $b"
                fi
            fi
        done
    fi

    if [ "$cword" -eq 1 ]; then
        COMPREPLY=($(compgen -W "$commands $profiles" -- "$cur"))
        return 0
    fi

    case "${words[1]}" in
        switch|run|usage|quota|disable|enable|remove|rm|delete|map)
            COMPREPLY=($(compgen -W "$profiles" -- "$cur"))
            return 0
            ;;
        completions)
            COMPREPLY=($(compgen -W "bash zsh fish powershell" -- "$cur"))
            return 0
            ;;
        config)
            COMPREPLY=($(compgen -W "get set unset path list" -- "$cur"))
            return 0
            ;;
        *)
            ;;
    esac
}

complete -F _cuser cuser claude-user
"#;

const FISH_COMPLETIONS: &str = r#"function __cuser_profiles
    set -l root (test -n "$CLAUDE_PROFILES_DIR"; and echo "$CLAUDE_PROFILES_DIR"; or echo "$HOME/.claude-profiles")
    if test -d "$root"
        for d in $root/*
            if test -d "$d"
                set -l base (basename "$d")
                if test "$base" != "shared"
                    echo "$base"
                end
            end
        end
    end
end

complete -c cuser -n "__fish_use_subcommand" -a switch -d "Switch active profile without launching"
complete -c cuser -n "__fish_use_subcommand" -a run -d "Launch isolated session mode"
complete -c cuser -n "__fish_use_subcommand" -a list -d "List profiles with usage limits"
complete -c cuser -n "__fish_use_subcommand" -a usage -d "Show detailed quota and rate limit breakdown"
complete -c cuser -n "__fish_use_subcommand" -a auto -d "Auto-rotate accounts before hitting rate limits"
complete -c cuser -n "__fish_use_subcommand" -a alias -d "Manage profile aliases"
complete -c cuser -n "__fish_use_subcommand" -a unalias -d "Remove a profile alias"
complete -c cuser -n "__fish_use_subcommand" -a config -d "View and edit tool settings"
complete -c cuser -n "__fish_use_subcommand" -a add-token -d "Register profile from token or API key"
complete -c cuser -n "__fish_use_subcommand" -a export -d "Export profiles to backup JSON"
complete -c cuser -n "__fish_use_subcommand" -a import -d "Import backup file or ~/.claude"
complete -c cuser -n "__fish_use_subcommand" -a import-usage -d "Import usage snapshot readings"
complete -c cuser -n "__fish_use_subcommand" -a current -d "Show the active profile"
complete -c cuser -n "__fish_use_subcommand" -a status -d "Show the active profile"
complete -c cuser -n "__fish_use_subcommand" -a map -d "Bind a directory to a profile"
complete -c cuser -n "__fish_use_subcommand" -a unmap -d "Remove a directory mapping"
complete -c cuser -n "__fish_use_subcommand" -a disable -d "Hold a profile out of rotation"
complete -c cuser -n "__fish_use_subcommand" -a enable -d "Re-enable a disabled profile"
complete -c cuser -n "__fish_use_subcommand" -a sync -d "Sync shared config"
complete -c cuser -n "__fish_use_subcommand" -a remove -d "Delete a profile"
complete -c cuser -n "__fish_use_subcommand" -a rename -d "Rename a profile"
complete -c cuser -n "__fish_use_subcommand" -a purge -d "Remove all claude-user data"
complete -c cuser -n "__fish_use_subcommand" -a completions -d "Generate shell autocompletions"

complete -c cuser -n "__fish_use_subcommand" -a "(__cuser_profiles)" -d "Profile"

complete -c cuser -n "__fish_seen_subcommand_from switch run usage disable enable remove rename map" -a "(__cuser_profiles)"
complete -c cuser -n "__fish_seen_subcommand_from completions" -a "bash zsh fish powershell"
complete -c cuser -n "__fish_seen_subcommand_from config" -a "get set unset path list"

complete -c claude-user -w cuser
"#;

const PWSH_COMPLETIONS: &str = r#"Register-ArgumentCompleter -Native -CommandName @('cuser', 'claude-user') -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $subcommands = @(
        [Management.Automation.CompletionResult]::new('switch', 'switch', 'ParameterValue', 'Switch active profile without launching'),
        [Management.Automation.CompletionResult]::new('run', 'run', 'ParameterValue', 'Launch isolated session mode'),
        [Management.Automation.CompletionResult]::new('list', 'list', 'ParameterValue', 'List existing profiles with quota'),
        [Management.Automation.CompletionResult]::new('usage', 'usage', 'ParameterValue', 'Show detailed quota and rate limit breakdown'),
        [Management.Automation.CompletionResult]::new('auto', 'auto', 'ParameterValue', 'Auto-rotate accounts before hitting rate limits'),
        [Management.Automation.CompletionResult]::new('alias', 'alias', 'ParameterValue', 'Assign or view short profile aliases'),
        [Management.Automation.CompletionResult]::new('unalias', 'unalias', 'ParameterValue', 'Remove a profile alias'),
        [Management.Automation.CompletionResult]::new('config', 'config', 'ParameterValue', 'View and edit tool settings'),
        [Management.Automation.CompletionResult]::new('add-token', 'add-token', 'ParameterValue', 'Register profile from token or API key'),
        [Management.Automation.CompletionResult]::new('export', 'export', 'ParameterValue', 'Export profiles to backup JSON'),
        [Management.Automation.CompletionResult]::new('import', 'import', 'ParameterValue', 'Import backup file or ~/.claude'),
        [Management.Automation.CompletionResult]::new('import-usage', 'import-usage', 'ParameterValue', 'Import usage snapshot readings'),
        [Management.Automation.CompletionResult]::new('current', 'current', 'ParameterValue', 'Show the active profile'),
        [Management.Automation.CompletionResult]::new('status', 'status', 'ParameterValue', 'Show the active profile'),
        [Management.Automation.CompletionResult]::new('map', 'map', 'ParameterValue', 'Bind a directory to a profile'),
        [Management.Automation.CompletionResult]::new('unmap', 'unmap', 'ParameterValue', 'Remove a directory mapping'),
        [Management.Automation.CompletionResult]::new('disable', 'disable', 'ParameterValue', 'Hold a profile out of rotation'),
        [Management.Automation.CompletionResult]::new('enable', 'enable', 'ParameterValue', 'Re-enable a disabled profile'),
        [Management.Automation.CompletionResult]::new('sync', 'sync', 'ParameterValue', 'Copy shared config into every existing profile'),
        [Management.Automation.CompletionResult]::new('remove', 'remove', 'ParameterValue', 'Delete a profile'),
        [Management.Automation.CompletionResult]::new('rename', 'rename', 'ParameterValue', 'Rename a profile'),
        [Management.Automation.CompletionResult]::new('purge', 'purge', 'ParameterValue', 'Remove all claude-user data'),
        [Management.Automation.CompletionResult]::new('completions', 'completions', 'ParameterValue', 'Generate shell autocompletions')
    )

    $subcommands | Where-Object { $_.CompletionText -like "$wordToComplete*" }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_print_completions_supported() {
        assert!(print_completions("bash").is_ok());
        assert!(print_completions("zsh").is_ok());
        assert!(print_completions("fish").is_ok());
        assert!(print_completions("powershell").is_ok());
        assert!(print_completions("pwsh").is_ok());
        assert!(print_completions("invalid").is_err());
    }
}

