#!/usr/bin/env bash
# TODO: this is a prototype for ebuild execution

# Those functions are implemented in src/ebuild/handler/functions
funcs="ver_cut ver_rs ver_test inherit"
for x in $funcs; do
    eval "
        $x() {
            printf 'FN %s %s\n' \"\$FUNCNAME\" \"\$*\" >&11
            local reply
            IFS= read -r reply <&10 || exit 1
            printf '%s\n' \"\$reply\"
        }
    "
done
ls -la /proc/$$/fd
printenv
whoami
#read -r line <&3
#echo "bash got: ${line}" >&6

# shellcheck source=/dev/null
source "${EBUILD}"
