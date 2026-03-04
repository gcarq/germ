# This is a stub function to call functions in the parent process via IPC.
# The format is as follows: FN\0function_name\0arg1\0arg2\0...\4
# So a hardcoded FN identifier, followed by the function name and arguments, everything is delimited by \0 and the
# message is terminated with \4.
__ipc_call() {
    {
        printf 'FN\0%s' "$1"
        shift
        for arg in "$@"; do
            printf '\0%s' "$arg"
        done
        printf '\4'
    } >&"${CHILD_WRITE_FD}" || exit 2

    local reply
    IFS= read -r reply <&"${CHILD_READ_FD}" || exit 1

    case ${reply} in
        OK) return 0 ;;
        OK\ *) printf '%s\n' "${reply#OK }"; return 0 ;;
        ERR) return 1 ;;
        ERR\ *) printf '%s\n' "${reply#ERR }" >&2; return 1 ;;
        *) printf 'protocol error: %s\n' "${reply}" >&2; exit 2 ;;
    esac
}

# Same as __ipc_call, but for sending data instead of calling a function.
# The format is: DATA\0KEY=value\4
__ipc_data() {
    printf 'DATA\0%s=%s\4' "$1" "$2" >&"${CHILD_WRITE_FD}" || exit 2
}

__resolve_eclass() { __ipc_call __resolve_eclass "$@"; }
contains_word()    { __ipc_call contains_word    "$@"; }
die()              { __ipc_call die              "$@"; }
has()              { __ipc_call has              "$@"; }
hasv()             { __ipc_call hasv             "$@"; }
hasq()             { __ipc_call hasq             "$@"; }
ver_cut()          { __ipc_call ver_cut          "$@"; }
ver_rs()           { __ipc_call ver_rs           "$@"; }
ver_test()         { __ipc_call ver_test         "$@"; }


# Debugging functions.
# If EBUILD_DEBUG is not set to 1, these functions do nothing.
if [[ "${EBUILD_DEBUG}" == 1 ]]; then
    debug-print()          { __ipc_call debug-print      "$@"; }
    debug-print-function() { debug-print "${1}: entering function, parameters: ${*:2}"; }
    debug-print-section()  { debug-print "now in section ${*}"; }
else
    debug-print()          { :; }
    debug-print-function() { :; }
    debug-print-section()  { :; }
fi