#!/usr/bin/env bash
set -o pipefail

# pulsar_integration_env.sh
#
# SUMMARY
#
#   Builds and pulls down the Vector Pulsar Integration test environment

if [ $# -ne 1 ]
then
    echo "Usage: $0 {stop|start}" 1>&2; exit 1;
    exit 1
fi
ACTION=$1

#
# Functions
#

start_podman () {
  "${CONTAINER_TOOL}" pod create --replace --name vector-test-integration-pulsar -p 6650:6650
  "${CONTAINER_TOOL}" run -d --pod=vector-test-integration-pulsar  --name vector_pulsar \
	 apachepulsar/pulsar bin/pulsar standalone
}

start_docker () {
   "${CONTAINER_TOOL}" network create vector-test-integration-pulsar
  "${CONTAINER_TOOL}" run -d --network=vector-test-integration-pulsar -p 6650:6650 --name vector_pulsar \
	 apachepulsar/pulsar bin/pulsar standalone
}

stop_podman () {
  "${CONTAINER_TOOL}" rm --force vector_pulsar 2>/dev/null; true
  "${CONTAINER_TOOL}" pod stop vector-test-integration-pulsar 2>/dev/null; true
  "${CONTAINER_TOOL}" pod rm --force vector-test-integration-pulsar 2>/dev/null; true
}

stop_docker () {
  "${CONTAINER_TOOL}" rm --force vector_pulsar 2>/dev/null; true
  "${CONTAINER_TOOL}" network rm vector-test-integration-pulsar 2>/dev/null; true
}

echo "Running $ACTION action for Pulsar integration tests environment"

"${ACTION}"_"${CONTAINER_TOOL}"
