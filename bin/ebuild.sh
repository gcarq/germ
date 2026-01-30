#!/usr/bin/env bash
# TODO: this is a prototype for ebuild execution


funcs="ver_cut inherit llvm_gen_dep"
for x in ${funcs} ; do
    eval "${x} () { echo \"executing: '\${FUNCNAME} \$*'\"; exit 1; }"
done

ls -la /proc/$$/fd
printenv
whoami
#read -r line <&3
#echo "bash got: ${line}" >&6

# shellcheck source=/dev/null
source "${EBUILD}"
