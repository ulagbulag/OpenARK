#!/bin/bash
# Copyright (c) 2024 Ho Kim (ho.kim@ulagbulag.io). All rights reserved.
# Use of this source code is governed by a GPL-3-style license that can be
# found in the LICENSE file.

# Prehibit errors
set -e -o pipefail
# Verbose
set -x

###########################################################
#   Configuration                                         #
###########################################################

# Configure default environment variables
HELM_CHART_DEFAULT="https://ray-project.github.io/kuberay-helm/"
NAMESPACE_DEFAULT="ai"

# Set environment variables
HELM_CHART="${HELM_CHART:-$HELM_CHART_DEFAULT}"
NAMESPACE="${NAMESPACE:-$NAMESPACE_DEFAULT}"

###########################################################
#   Configure Helm Channel                                #
###########################################################

echo "- Configuring Helm channel ... "

helm repo add "${NAMESPACE}-kuberay" "${HELM_CHART}"

###########################################################
#   Checking if Operator is already installed             #
###########################################################

echo "- Checking Operator is already installed ... "
if
    kubectl get namespace --no-headers "${NAMESPACE}" \
        >/dev/null 2>/dev/null
then
    IS_FIRST=0
else
    IS_FIRST=1
fi

###########################################################
#   Install KubeRay Cluster                               #
###########################################################

echo "- Installing KubeRay Cluster ... "

helm upgrade --install "ray-cluster" \
    "${NAMESPACE}-kuberay/ray-cluster" \
    --create-namespace \
    --namespace "${NAMESPACE}" \
    --values "./values-cluster.yaml"

# Finished!
echo "Installed!"
