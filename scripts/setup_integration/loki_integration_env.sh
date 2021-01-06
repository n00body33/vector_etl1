#!/usr/bin/env bash
set -uo pipefail

# loki_integration_env.sh
#
# SUMMARY
#
#   Builds and pulls down the Vector Loki Integration test environment

set -x

while getopts a:t:e: flag
do
    case "${flag}" in
        a) action=${OPTARG};;
        t) tool=${OPTARG};;
        e) enclosure=${OPTARG};;

    esac
done

ACTION="${action:-"stop"}"
CONTAINER_TOOL="${tool:-"podman"}"
CONTAINER_ENCLOSURE="${enclosure:-"pod"}"

#
# Functions
#

start_podman () {
	"${CONTAINER_TOOL}" "${CONTAINER_ENCLOSURE}" create --replace --name vector-test-integration-loki -p 3100:3100
	"${CONTAINER_TOOL}" run -d --"${CONTAINER_ENCLOSURE}"=vector-test-integration-loki -v "$(PWD)"/tests/data:/etc/loki \
	 --name vector_loki grafana/loki:master -config.file=/etc/loki/loki-config.yaml
}

start_docker () {
	"${CONTAINER_TOOL}" "${CONTAINER_ENCLOSURE}" create vector-test-integration-loki
	"${CONTAINER_TOOL}" run -d --"${CONTAINER_ENCLOSURE}"=vector-test-integration-loki -p 3100:3100 -v "$(PWD)"/tests/data:/etc/loki \
	 --name vector_loki grafana/loki:master -config.file=/etc/loki/loki-config.yaml
}

stop_podman () {
	"${CONTAINER_TOOL}" rm --force vector_loki 2>/dev/null; true
  "${CONTAINER_TOOL}" "${CONTAINER_ENCLOSURE}" stop vector-test-integration-loki 2>/dev/null; true
	"${CONTAINER_TOOL}" "${CONTAINER_ENCLOSURE}" rm --force vector-test-integration-loki 2>/dev/null; true
}

stop_docker () {
	"${CONTAINER_TOOL}" rm --force vector_loki 2>/dev/null; true
	"${CONTAINER_TOOL}" "${CONTAINER_ENCLOSURE}" rm vector-test-integration-loki 2>/dev/null; true
}

echo "Running $ACTION action for Loki integration tests environment"

"${ACTION}"_"${CONTAINER_TOOL}"
