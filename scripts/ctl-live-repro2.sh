#!/bin/sh
# Run inside a Termdeck pane. It drives a second pane through every Phase-2
# control verb; --force keeps the demonstration independent of the input gate.
set -eu

: "${TERMDECK_SOCK:?run this inside a Termdeck pane}"
path=${1:-"$PWD"}

opened=$(termctl --json open "$path" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
if [ -z "$opened" ]; then
    echo "open did not return a pane id" >&2
    exit 1
fi

termctl input "$opened" --text 'printf ctl-live-repro2' --force
termctl peek "$opened" 10
termctl promote "$opened"
termctl zoom --on
termctl zoom --off
termctl close "$opened"
