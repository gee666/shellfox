__shellfox_status=$?
[[ -n ${__shellfox_completion-} ]] || {
    __shellfox_completion=1
    complete -p shellfox &>/dev/null || {
@COMPLETION@
    } 2>/dev/null
}
