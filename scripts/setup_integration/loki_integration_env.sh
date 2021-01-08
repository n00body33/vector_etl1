#!/usr/bin/env bash
set -o pipefail

# loki_integration_env.sh
#
# SUMMARY
#
#   Builds and pulls down the Vector Loki Integration test environment

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
  "${CONTAINER_TOOL}" pod create --replace --name vector-test-integration-loki -p 3100:3100
  "${CONTAINER_TOOL}" run -d --pod=vector-test-integration-loki -v "$(pwd)"/tests/data:/etc/loki \
	 --name vector_loki grafana/loki:master -config.file=/etc/loki/loki-config.yaml
}

start_docker () {
   "${CONTAINER_TOOL}" network create vector-test-integration-loki
  "${CONTAINER_TOOL}" run -d --network=vector-test-integration-loki -p 3100:3100 -v "$(pwd)"/tests/data:/etc/loki \
	 --name vector_loki grafana/loki:master -config.file=/etc/loki/loki-config.yaml
}

stop_podman () {
  "${CONTAINER_TOOL}" rm --force vector_loki 2>/dev/null; true
  "${CONTAINER_TOOL}" pod stop vector-test-integration-loki 2>/dev/null; true
  "${CONTAINER_TOOL}" pod rm --force vector-test-integration-loki 2>/dev/null; true
}

stop_docker () {
  "${CONTAINER_TOOL}" rm --force vector_loki 2>/dev/null; true
  "${CONTAINER_TOOL}" network rm vector-test-integration-loki 2>/dev/null; true
}

echo "Running $ACTION action for Loki integration tests environment"

"${ACTION}"_"${CONTAINER_TOOL}"
