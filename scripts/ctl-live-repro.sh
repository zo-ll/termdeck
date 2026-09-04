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
