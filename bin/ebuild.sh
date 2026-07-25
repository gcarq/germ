#!/usr/bin/env bash
# TODO: this is a prototype for ebuild execution

# Prevent aliases from causing portage to act inappropriately.
# Make sure it's before everything so we don't mess aliases that follow.
unalias -a

source "./bin/eapi.sh" || exit
source "./bin/functions.sh" || exit

if [[ ${EBUILD_PHASE} == depend ]]; then
    __depend_phase_forbidden() {
        die "${FUNCNAME[1]}() calls are not allowed in global scope"
    }

    # These dummy functions are for things that are likely to be called
    # in global scope, even though they are completely useless during
    # the "depend" phase.
    diropts() { __depend_phase_forbidden "$@"; }
    docompress() { __depend_phase_forbidden "$@"; }
    dostrip() { __depend_phase_forbidden "$@"; }
    exeopts() { __depend_phase_forbidden "$@"; }
    get_KV() { __depend_phase_forbidden "$@"; }
    insopts() { __depend_phase_forbidden "$@"; }
    KV_major() { __depend_phase_forbidden "$@"; }
    KV_micro() { __depend_phase_forbidden "$@"; }
    KV_minor() { __depend_phase_forbidden "$@"; }
    KV_to_int() { __depend_phase_forbidden "$@"; }
    in_iuse() { __depend_phase_forbidden "$@"; }
    get_libdir() { __depend_phase_forbidden "$@"; }
    libopts() { __depend_phase_forbidden "$@"; }
    register_die_hook() { __depend_phase_forbidden "$@"; }
    register_success_hook() { __depend_phase_forbidden "$@"; }
    __strip_duplicate_slashes() { __depend_phase_forbidden "$@"; }
    use() { __depend_phase_forbidden "$@"; }
    useq() { __depend_phase_forbidden "$@"; }
    usev() { __depend_phase_forbidden "$@"; }
    usex() { __depend_phase_forbidden "$@"; }
    use_with() { __depend_phase_forbidden "$@"; }
    use_enable() { __depend_phase_forbidden "$@"; }
    # These functions die because calls to them during the "depend" phase
    # are considered to be severe QA violations.
    best_version() { __depend_phase_forbidden "$@"; }
    has_version() { __depend_phase_forbidden "$@"; }
    portageq() { __depend_phase_forbidden "$@"; }

    # prevent the shell from finding external executables
    # note: we can't use empty because it implies current directory
    export PATH=/dev/null
    command_not_found_handle() {
        die "External commands disallowed while sourcing ebuild: ${*}"
    }
fi

# Don't use sandbox's BASH_ENV for new shells because it does
# 'source /etc/profile' which can interfere with the build
# environment by modifying our PATH.
unset BASH_ENV

# Exports stub functions that call the eclass's functions, thereby making them default.
# For example, if ECLASS="base" and you call "EXPORT_FUNCTIONS src_unpack", the following
# code will be eval'd:
# src_unpack() { base_src_unpack; }
EXPORT_FUNCTIONS() {
    if [[ -z "${ECLASS}" ]]; then
        die "EXPORT_FUNCTIONS without a defined ECLASS"
    fi
    eval ${__export_funcs_var}+=\" $*\"
}

trap 'exit 1' SIGTERM

eval "PORTAGE_ECLASS_LOCATIONS=(${PORTAGE_ECLASS_LOCATIONS})"


if [[ ${EBUILD_PHASE} == depend ]]; then

    # *DEPEND and IUSE will be set during the sourcing of the ebuild.
    # In order to ensure correct interaction between ebuilds and
    # eclasses, they need to be unset before this process of
    # interaction begins.
    unset EAPI DEPEND RDEPEND PDEPEND BDEPEND PROPERTIES RESTRICT
    unset INHERITED IUSE REQUIRED_USE ECLASS E_IUSE E_REQUIRED_USE
    unset E_DEPEND E_RDEPEND E_PDEPEND E_BDEPEND E_IDEPEND E_PROPERTIES
    unset E_RESTRICT PROVIDES_EXCLUDE REQUIRES_EXCLUDE
    unset PORTAGE_EXPLICIT_INHERIT

	pre_source_sandbox() {
		export SANDBOX_ON=1

		export SANDBOX_PREDICT=""
		export SANDBOX_READ="${EBUILD}:${BASH_SOURCE[1]}:${SANDBOX_LOG}"
		export SANDBOX_WRITE="${SANDBOX_LOG}:/dev/null"
		[[ ${EBUILD_DEBUG} != 1 ]] || export SANDBOX_DEBUG=1

		# We need inherit to work
		local repo_location
		for repo_location in "${PORTAGE_ECLASS_LOCATIONS[@]}"; do
			local potential_location="${repo_location}/eclass"
			if [[ -d ${potential_location} ]]; then
				SANDBOX_READ+=":${potential_location}"
			fi
		done

		# Give a nicer error message in case someone is confused
		adddeny() { die "External commands disallowed while sourcing ebuild: ${FUNCNAME}" ; }
		addpredict() { die "External commands disallowed while sourcing ebuild: ${FUNCNAME}" ; }
		addread() { die "External commands disallowed while sourcing ebuild: ${FUNCNAME}" ; }
		addwrite() { die "External commands disallowed while sourcing ebuild: ${FUNCNAME}" ; }

		# Disallow any tampering
		readonly SANDBOX_{ALLOW,ACTIVE,DENY,DEBUG,ON,PREDICT,READ,WRITE}
	}
	pre_source_sandbox

    # shellcheck source=/dev/null
    source "${EBUILD}" || die "error sourcing ebuild"

    [[ -s ${SANDBOX_LOG} ]] && die "Sandbox violations found, exiting"

    # Add in dependency info from eclasses
    IUSE+="${IUSE:+ }${E_IUSE}"
    DEPEND+="${DEPEND:+ }${E_DEPEND}"
    RDEPEND+="${RDEPEND:+ }${E_RDEPEND}"
    PDEPEND+="${PDEPEND:+ }${E_PDEPEND}"
    BDEPEND+="${BDEPEND:+ }${E_BDEPEND}"
    IDEPEND+="${IDEPEND:+ }${E_IDEPEND}"
    REQUIRED_USE+="${REQUIRED_USE:+ }${E_REQUIRED_USE}"

    if ___eapi_has_accumulated_PROPERTIES; then
        PROPERTIES+=${PROPERTIES:+ }${E_PROPERTIES}
    fi
    if ___eapi_has_accumulated_RESTRICT; then
        RESTRICT+=${RESTRICT:+ }${E_RESTRICT}
    fi

    # alphabetically ordered by ${EBUILD_PHASE} value
    _valid_phases="src_compile pkg_config src_configure pkg_info
        src_install pkg_nofetch pkg_postinst pkg_postrm pkg_preinst
        src_prepare pkg_prerm pkg_pretend pkg_setup src_test src_unpack"
    DEFINED_PHASES=
    for _f in ${_valid_phases}; do
        if declare -F ${_f} >/dev/null; then
            _f=${_f#pkg_}
            DEFINED_PHASES+=" ${_f#src_}"
        fi
    done
    [[ -n ${DEFINED_PHASES} ]] || DEFINED_PHASES=-
    unset _f _valid_phases

    __send_metadata

else
    die "TODO: ${EBUILD_PHASE} is not implemented"
fi
