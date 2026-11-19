# fish completion for r
#
# Install it with:
#   r completions fish > ~/.config/fish/completions/r.fish

function __r_complete
    set -l tokens (commandline -opc) (commandline -ct)
    set -e tokens[1]
    set -l candidates (command r __complete fish -- $tokens 2>/dev/null)
    if test (count $candidates) -gt 0
        printf '%s\n' $candidates
    else
        # Without candidates (e.g. for a task's own arguments) complete file names.
        __fish_complete_path (commandline -ct)
    end
end

complete -c r -f -a '(__r_complete)'
