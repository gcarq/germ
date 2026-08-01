# This is a stub function to call functions in the parent process via IPC.
#
# The format is as follows: `FN\0function_name\0arg1\0arg2\0...\4`
# This means, a hardcoded `FN` identifier, followed by the function name and arguments.
# Everything is delimited by `\0` and the message is terminated with `\4`.
#
# NOTE: The result of the function call is also stored in `__ipc_reply`.
__ipc_call() {
    {
        printf 'FN\0%s' "$1"
        shift
        for arg in "$@"; do
            printf '\0%s' "$arg"
        done
        printf '\4'
    } >&"${CHILD_WRITE_FD}" || exit 2

    IFS= read -r __ipc_reply <&"${CHILD_READ_FD}" || exit 1

    case ${__ipc_reply} in
    OK)
        __ipc_reply=
        return 0
        ;;
    OK\ *)
        __ipc_reply=${__ipc_reply#OK }
        printf '%s\n' "${__ipc_reply}"
        return 0
        ;;
    ERR)
        __ipc_reply=
        return 1
        ;;
    ERR\ *)
        printf '%s\n' "${__ipc_reply#ERR }" >&2
        __ipc_reply=
        return 1
        ;;
    DIE)
        __ipc_reply=
        exit 1
        ;;
    *)
        printf 'protocol error: %s\n' "${__ipc_reply}" >&2
        exit 2
        ;;
    esac
}

# Same as __ipc_call, but for sending data instead of calling a function.
# The format is: DATA\0KEY=value\4
__ipc_data() {
    printf 'DATA\0%s=%s\4' "$1" "$2" >&"${CHILD_WRITE_FD}" || exit 2
}

# Sends the metadata gathered in the DEPEND phase to the parent process.
__send_metadata() {
    metadata_keys=(
        DEPEND RDEPEND SLOT SRC_URI RESTRICT HOMEPAGE LICENSE
        DESCRIPTION KEYWORDS INHERITED IUSE REQUIRED_USE PDEPEND BDEPEND
        EAPI PROPERTIES DEFINED_PHASES IDEPEND INHERIT
    )

    if ! ___eapi_has_IDEPEND; then
        unset IDEPEND
    fi

    INHERIT=${PORTAGE_EXPLICIT_INHERIT}

    # Send metadata as single-line KEY=value pairs
    for key in "${metadata_keys[@]}"; do
        value=${!key-}
        value=${value//$'\n'/ }
        __ipc_data "${key}" "${value}"
    done
    exec {CHILD_WRITE_FD}>&-
}
