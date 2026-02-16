#!/usr/bin/env bash


# VARIABLES

___eapi_has_accumulated_PROPERTIES() [[ ${1-${EAPI-0}} != [0-7] ]]
___eapi_has_accumulated_RESTRICT() [[ ${1-${EAPI-0}} != [0-7] ]]