#!/bin/sh
# Runs a command against a private GTK Broadway display, so no window reaches the desktop.
# Usage: dev/headless.sh cargo test
set -eu

number="${HEADLESS_DISPLAY:-5}"
display=":$number"
# Display :N listens on broadway<N+1>.socket.
socket="${XDG_RUNTIME_DIR:-/tmp}/broadway$((number + 1)).socket"

# Bind the daemon's web viewer to loopback only.
gtk4-broadwayd --address 127.0.0.1 "$display" >/dev/null 2>&1 &
daemon=$!
trap 'kill "$daemon" 2>/dev/null || true' EXIT INT TERM

tries=0
until [ -S "$socket" ]; do
    tries=$((tries + 1))
    if [ "$tries" -gt 50 ]; then
        echo "gtk4-broadwayd did not start on $display" >&2
        exit 1
    fi
    sleep 0.1
done

GDK_BACKEND=broadway BROADWAY_DISPLAY="$display" "$@"
