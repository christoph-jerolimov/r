# bash completion for r
#
# Load it from ~/.bashrc:
#   eval "$(r completions bash)"

_r_complete() {
    local IFS=$'\n'
    # r gets the raw line and splits it itself: bash breaks words at ":", which
    # would cut task names such as "build:prod" into pieces.
    COMPREPLY=($(command r __complete bash \
        --line "${COMP_LINE:0:COMP_POINT}" \
        --wordbreaks "$COMP_WORDBREAKS" 2>/dev/null))
}

# Without candidates (e.g. for a task's own arguments) fall back to file names.
complete -o bashdefault -o default -F _r_complete r
