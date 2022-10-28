#!/bin/bash
# Copyright (c) 2022 Ho Kim (ho.kim@ulagbulag.io). All rights reserved.
# Use of this source code is governed by a GPL-3-style license that can be
# found in the LICENSE file.

# Prehibit errors
set -e

###########################################################
#   Benchmark Configuration                               #
###########################################################

# Configure default environment variables
ADDRESS_READ_DEFAULT="127.0.0.1"
ADDRESS_WRITE_DEFAULT="127.0.0.1"
DATA_SIZE_DEFAULT="1Ki 4Ki 16Ki 64Ki 256Ki 1Mi 4Mi 16Mi 64Mi 256Mi"
NUM_ITERATIONS_DEFAULT="1K"
NUM_THREADS_DEFAULT="8"
PORT_DEFAULT="none 9802 9803 9804" # available options: [none, 9802, 9803, 9804]
PROTOCOL_DEFAULT="ipiis"           # available options: [ipiis, ipfs, local, s3]
SIMULATION_DELAY_MS_DEFAULT="0 50 100"

# Configure environment variables
ADDRESS_READ="${ADDRESS_READ:-$ADDRESS_READ_DEFAULT}"
ADDRESS_WRITE="${ADDRESS_WRITE:-$ADDRESS_WRITE_DEFAULT}"
DATA_SIZE="${DATA_SIZE:-$DATA_SIZE_DEFAULT}"
NUM_ITERATIONS="${NUM_ITERATIONS:-$NUM_ITERATIONS_DEFAULT}"
NUM_THREADS="${NUM_THREADS:-$NUM_THREADS_DEFAULT}"
PORT="${PORT:-$PORT_DEFAULT}"
PROTOCOL="${PROTOCOL:-$PROTOCOL_DEFAULT}"
SIMULATION_DELAY_MS="${SIMULATION_DELAY_MS:-$SIMULATION_DELAY_MS_DEFAULT}"

###########################################################
#   Configuration                                         #
###########################################################

# Configure default environment variables
SAVE_DIR_DEFAULT="./results"

# Configure environment variables
SAVE_DIR="${SAVE_DIR:-$SAVE_DIR_DEFAULT}"

###########################################################
#   Install Bench Tools                                   #
###########################################################

echo "- Installing ipis bench tools ... "
kubectl apply \
    -f "./bench-tools.yaml" \
    >/dev/null

echo -n "- Waiting for deploying Ceph Tools ... "
kubectl --namespace "ipis" rollout status deployment "ipsis-bench-tools" >/dev/null
echo "OK"

###########################################################
#   DO Benchmark IPSIS                                    #
###########################################################

echo "- Starting benchmark ... "

for address_read in $ADDRESS_READ; do
    for address_write in $ADDRESS_WRITE; do
        for data_size in $DATA_SIZE; do
            for num_iterations in $NUM_ITERATIONS; do
                for num_threads in $NUM_THREADS; do
                    for port in $PORT; do
                        for protocol in $PROTOCOL; do
                            # skip if port and protocol is XOR
                            if [ "$port" == "none" ]; then
                                if [ "$protocol" == "ipiis" ]; then
                                    continue
                                fi
                            elif [ "$protocol" != "ipiis" ]; then
                                continue
                            fi

                            # skip if address_read is given and protocol is not IPFS
                            if [ "$address_read" != "$address_write" ]; then
                                if [ "$protocol" != "ipfs" ]; then
                                    continue
                                fi
                            fi

                            for simulation_delay_ms in $SIMULATION_DELAY_MS; do
                                # edit null-valued  variables
                                if [ "$port" == "none" ]; then
                                    port="9801"
                                fi

                                # print options
                                echo -n "ADDRESS_READ=$address_read | "
                                echo -n "ADDRESS_WRITE=$address_write | "
                                echo -n "DATA_SIZE=$data_size | "
                                echo -n "NUM_ITERATIONS=$num_iterations | "
                                echo -n "NUM_THREADS=$num_threads | "
                                echo -n "PORT=$port | "
                                echo -n "PROTOCOL=$protocol | "
                                echo -n "SIMULATION_DELAY_MS=$simulation_delay_ms | "

                                # do benchmark
                                cat "./bench.yaml" |
                                    sed "s/__ADDRESS_READ__/$address_read/g" |
                                    sed "s/__ADDRESS_WRITE__/$address_write/g" |
                                    sed "s/__DATA_SIZE__/$data_size/g" |
                                    sed "s/__NUM_ITERATIONS__/$num_iterations/g" |
                                    sed "s/__NUM_THREADS__/$num_threads/g" |
                                    sed "s/__PORT__/$port/g" |
                                    sed "s/__PROTOCOL__/$protocol/g" |
                                    sed "s/__SIMULATION_DELAY_MS__/$simulation_delay_ms/g" |
                                    kubectl apply -f - >/dev/null

                                # wait for completing
                                kubectl --namespace "ipis" wait "job/ipsis-bench" \
                                    --for=condition=complete \
                                    --timeout=-1s \
                                    >/dev/null

                                # remove the job
                                kubectl delete -f "./bench.yaml" >/dev/null
                                echo "OK"
                            done
                        done
                    done
                done
            done
        done
    done
done

###########################################################
#   Dump Results                                          #
###########################################################

echo -n "- Dumping results to \"$SAVE_DIR\" ... "
mkdir -p "$SAVE_DIR"
kubectl exec \
    --namespace "ipis" \
    "deployment/ipsis-bench-tools" -- \
    tar -cf - -C "/data/results/" "." |
    tar -xf - -C "$SAVE_DIR"
echo "OK"

# Finished!
echo "Installed!"