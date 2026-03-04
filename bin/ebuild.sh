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


# Sources all eclasses in parameters
declare -ix ECLASS_DEPTH=0
inherit() {
	ECLASS_DEPTH=$((${ECLASS_DEPTH} + 1))
	if [[ ${ECLASS_DEPTH} -gt 1 ]]; then
		debug-print "*** Multiple Inheritance (Level: ${ECLASS_DEPTH})"

		# Since ECLASS_DEPTH > 1, the following variables are locals from the
		# previous inherit call in the call stack.
		if [[ -n ${ECLASS} && -n ${!__export_funcs_var} ]] ; then
			eqawarn "QA Notice: EXPORT_FUNCTIONS is called before inherit in ${ECLASS}.eclass."
			eqawarn "For compatibility with PMS and to avoid breakage with Pkgcore, only call"
			eqawarn "EXPORT_FUNCTIONS after inherit(s). Portage behavior may change in future."
		fi
	fi

	local -x ECLASS
	local __export_funcs_var
	local location
	local x
	local B_IUSE
	local B_REQUIRED_USE
	local B_DEPEND
	local B_RDEPEND
	local B_PDEPEND
	local B_BDEPEND
	local B_IDEPEND
	local B_PROPERTIES
	local B_RESTRICT
	while [[ "${1}" ]]; do
		local location=""

		ECLASS="${1}"
		__export_funcs_var=__export_functions_${ECLASS_DEPTH}
		unset ${__export_funcs_var}

		if [[ ${EBUILD_PHASE} != depend && ${EBUILD_PHASE} != nofetch && \
			${EBUILD_PHASE} != *rm && ${EMERGE_FROM} != "binary" && \
			-z ${_IN_INSTALL_QA_CHECK} ]]
		then
			# This is disabled in the *rm phases because they frequently give
			# false alarms due to INHERITED in /var/db/pkg being outdated
			# in comparison to the eclasses from the ebuild repository. It's
			# disabled for nofetch, since that can be called by repoman and
			# that triggers bug #407449 due to repoman not exporting
			# non-essential variables such as INHERITED.
			if ! contains_word "${ECLASS}" "${INHERITED} ${__INHERITED_QA_CACHE}"; then
				eqawarn "QA Notice: Eclass '${ECLASS}' inherited illegally in ${CATEGORY}/${PF} ${EBUILD_PHASE}"
			fi
		fi

        location=$(__resolve_eclass "${ECLASS}")
		debug-print "inherit: ${1} -> ${location}"
		[[ -z ${location} ]] && die "${1}.eclass could not be found by inherit()"

		# Inherits in QA checks can't handle metadata assignments
		if [[ -z ${_IN_INSTALL_QA_CHECK} ]]; then
			# We need to back up the values of *DEPEND to B_*DEPEND
			# (if set).. and then restore them after the inherit call.

			# Turn off glob expansion
			set -f

			# Retain the old data and restore it later.
			unset B_IUSE B_REQUIRED_USE B_DEPEND B_RDEPEND B_PDEPEND
			unset B_BDEPEND B_IDEPEND B_PROPERTIES B_RESTRICT
			[[ -v IUSE         ]] && B_IUSE="${IUSE}"
			[[ -v REQUIRED_USE ]] && B_REQUIRED_USE="${REQUIRED_USE}"
			[[ -v DEPEND       ]] && B_DEPEND="${DEPEND}"
			[[ -v RDEPEND      ]] && B_RDEPEND="${RDEPEND}"
			[[ -v PDEPEND      ]] && B_PDEPEND="${PDEPEND}"
			[[ -v BDEPEND      ]] && B_BDEPEND="${BDEPEND}"
			[[ -v IDEPEND      ]] && B_IDEPEND="${IDEPEND}"
			unset IUSE REQUIRED_USE DEPEND RDEPEND PDEPEND BDEPEND IDEPEND

			if ___eapi_has_accumulated_PROPERTIES; then
				[[ -v PROPERTIES ]] && B_PROPERTIES=${PROPERTIES}
				unset PROPERTIES
			fi
			if ___eapi_has_accumulated_RESTRICT; then
				[[ -v RESTRICT ]] && B_RESTRICT=${RESTRICT}
				unset RESTRICT
			fi

			# Turn on glob expansion
			set +f
		fi

        # shellcheck source=/dev/null
		source "${location}" || die "died sourcing ${location} in inherit()"

		if [[ -z ${_IN_INSTALL_QA_CHECK} ]]; then
			# Turn off glob expansion
			set -f

			# If each var has a value, append it to the global variable E_* to
			# be applied after everything is finished. New incremental behavior.
			[[ -v IUSE         ]] && E_IUSE+="${E_IUSE:+ }${IUSE}"
			[[ -v REQUIRED_USE ]] && E_REQUIRED_USE+="${E_REQUIRED_USE:+ }${REQUIRED_USE}"
			[[ -v DEPEND       ]] && E_DEPEND+="${E_DEPEND:+ }${DEPEND}"
			[[ -v RDEPEND      ]] && E_RDEPEND+="${E_RDEPEND:+ }${RDEPEND}"
			[[ -v PDEPEND      ]] && E_PDEPEND+="${E_PDEPEND:+ }${PDEPEND}"
			[[ -v BDEPEND      ]] && E_BDEPEND+="${E_BDEPEND:+ }${BDEPEND}"
			[[ -v IDEPEND      ]] && E_IDEPEND+="${E_IDEPEND:+ }${IDEPEND}"

			[[ -v B_IUSE ]] && IUSE="${B_IUSE}"
			[[ -v B_IUSE ]] || unset IUSE

			[[ -v B_REQUIRED_USE ]] && REQUIRED_USE="${B_REQUIRED_USE}"
			[[ -v B_REQUIRED_USE ]] || unset REQUIRED_USE

			[[ -v B_DEPEND ]] && DEPEND="${B_DEPEND}"
			[[ -v B_DEPEND ]] || unset DEPEND

			[[ -v B_RDEPEND ]] && RDEPEND="${B_RDEPEND}"
			[[ -v B_RDEPEND ]] || unset RDEPEND

			[[ -v B_PDEPEND ]] && PDEPEND="${B_PDEPEND}"
			[[ -v B_PDEPEND ]] || unset PDEPEND

			[[ -v B_BDEPEND ]] && BDEPEND="${B_BDEPEND}"
			[[ -v B_BDEPEND ]] || unset BDEPEND

			[[ -v B_IDEPEND ]] && IDEPEND="${B_IDEPEND}"
			[[ -v B_IDEPEND ]] || unset IDEPEND

			if ___eapi_has_accumulated_PROPERTIES; then
				[[ -v PROPERTIES ]] &&
					E_PROPERTIES+=${E_PROPERTIES:+ }${PROPERTIES}
				[[ -v B_PROPERTIES ]] &&
					PROPERTIES=${B_PROPERTIES}
				[[ -v B_PROPERTIES ]] ||
					unset PROPERTIES
			fi
			if ___eapi_has_accumulated_RESTRICT; then
				[[ -v RESTRICT ]] &&
					E_RESTRICT+=${E_RESTRICT:+ }${RESTRICT}
				[[ -v B_RESTRICT ]] &&
					RESTRICT=${B_RESTRICT}
				[[ -v B_RESTRICT ]] ||
					unset RESTRICT
			fi

			# Turn on glob expansion
			set +f

			if [[ -n ${!__export_funcs_var} ]] ; then
				for x in ${!__export_funcs_var} ; do
					debug-print "EXPORT_FUNCTIONS: ${x} -> ${ECLASS}_${x}"
					declare -F "${ECLASS}_${x}" >/dev/null || \
						die "EXPORT_FUNCTIONS: ${ECLASS}_${x} is not defined"
					eval "$x() { ${ECLASS}_${x} \"\$@\" ; }" > /dev/null
				done
			fi
			unset $__export_funcs_var

			if ! contains_word "$1" "${INHERITED}"; then
				export INHERITED+=" $1"
			fi
			if [[ ${ECLASS_DEPTH} -eq 1 ]]; then
				export PORTAGE_EXPLICIT_INHERIT+=" $1"
			fi
		fi

		shift
	done
	((--ECLASS_DEPTH)) # Returns 1 when ECLASS_DEPTH reaches 0.
	return 0
}

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

# shellcheck source=/dev/null
source "${EBUILD}"

if [[ ${EBUILD_PHASE} = depend ]] ; then
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
fi
