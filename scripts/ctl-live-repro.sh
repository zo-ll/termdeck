#!/bin/sh
# Run this from one Termdeck pane while TARGET_PANE is producing output in
# another. It proves the in-pane TERMDECK_SOCK rendezvous and scrollback peek.
set -eu

: "${TERMDECK_SOCK:?run this inside a Termdeck pane}"
target=${1:?usage: sh scripts/ctl-live-repro.sh TARGET_PANE [LINES]}
lines=${2:-80}

termctl status
termctl list
termctl peek "$target" "$lines"

# The server responds with the ctl.v1 error envelope and exit code 2.
set +e
termctl --json peek "__termdeck_missing_pane__"
status=$?
set -e
if [ "$status" -ne 2 ]; then
    echo "expected missing-pane ctl exit 2, got $status" >&2
    exit 1
fi
