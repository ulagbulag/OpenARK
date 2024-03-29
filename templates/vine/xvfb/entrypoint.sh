#!/bin/sh
# Copyright (c) 2023 Ho Kim (ho.kim@ulagbulag.io). All rights reserved.
# Use of this source code is governed by a GPL-3-style license that can be
# found in the LICENSE file.

# Prehibit errors
set -e -o pipefail
# Verbose
set -x

###########################################################
#   Configuration                                         #
###########################################################

# Define default variables
ARGS="${XVFB_ARGS:-""}"

# Display
DISPLAY="${DISPLAY:-":0"}"
ARGS="${DISPLAY} ${ARGS}"

###########################################################
#   Execute program                                       #
###########################################################

exec Xvfb ${ARGS}
