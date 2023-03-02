#!/bin/bash
# Copyright (c) 2023 Ho Kim (ho.kim@ulagbulag.io). All rights reserved.
# Use of this source code is governed by a GPL-3-style license that can be
# found in the LICENSE file.

# Prehibit errors
set -e
# Verbose
set -x

# Catch trap signals
trap "echo 'Gracefully terminating...'; exit" INT TERM
trap "echo 'Terminated.'; exit" EXIT

# Initialize desktop environment
"$(dirname "$0")/init-desktop.sh"

# Execute a desktop environment
"$(dirname "$0")/init-desktop-xfce4.sh"