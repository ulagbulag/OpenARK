#!/bin/bash
# Copyright (c) 2023 Ho Kim (ho.kim@ulagbulag.io). All rights reserved.
# Use of this source code is governed by a GPL-3-style license that can be
# found in the LICENSE file.

# Prehibit errors
set -e -o pipefail
# Verbose
set -x

###########################################################
#   Install Templates                                     #
###########################################################

echo "- Installing templates ... "

for dir in ./*; do
    # playbook directory
    if [ -d "${dir}" ]; then
        kubectl create configmap "matchbox-$(basename ${dir})" \
            --namespace=kiss \
            --from-file=${dir} \
            --output=yaml \
            --dry-run=client |
            kubectl apply -f -
    fi
done

echo "- Restarting matchbox ... "
kubectl --namespace kiss rollout restart deploy matchbox

# Finished!
echo "Installed!"
