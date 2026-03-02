# This is a stub function to call functions in the parent process via IPC.
# The first argument is the function name, and the rest are the arguments to pass to that function.
__ipc_call() {
    {
        printf 'FN %s' "$1"
        shift
        for arg in "$@"; do
            printf ' %q' "$arg"
        done
        printf '\n'
    } >&11

    local reply
    IFS= read -r reply <&10 || exit 1

    case ${reply} in
        OK) return 0 ;;
        OK\ *) printf '%s\n' "${reply#OK }"; return 0 ;;
        ERR) return 1 ;;
        ERR\ *) printf '%s\n' "${reply#ERR }" >&2; return 1 ;;
        *) printf 'protocol error: %s\n' "${reply}" >&2; exit 2 ;;
    esac
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