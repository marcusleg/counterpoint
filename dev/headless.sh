#!/bin/sh
# Runs a command against a private GTK Broadway display, so no window reaches the desktop.
# Usage: dev/headless.sh cargo test
set -eu

number="${HEADLESS_DISPLAY:-5}"
display=":$number"
# Display :N listens on broadway<N+1>.socket.
socket="${XDG_RUNTIME_DIR:-/tmp}/broadway$((number + 1)).socket"

# Set before the traps are installed below, so cleanup can never see an unbound variable if a
# signal arrives before the daemon is started or its log file is created.
daemon=
log=

cleanup() {
    kill "${daemon:-}" 2>/dev/null || true
    # gtk4-broadwayd does not delete its socket when killed.
    rm -f "$socket" "${log:-}"
}
trap cleanup EXIT
# Handled separately from EXIT so Ctrl+C (or a TERM) during the wait loop below runs cleanup and
# exits right away, with the conventional 128+signal status, instead of falling through to the
# loop's own "exited before creating" message.
trap 'cleanup; exit 130' INT
trap 'cleanup; exit 143' TERM

log="$(mktemp)"

# A socket left behind by a killed daemon would make the wait loop below pass instantly while
# the new daemon is still starting, or fails to bind.
rm -f "$socket"

# Bind the daemon's web viewer to loopback only.
gtk4-broadwayd --address 127.0.0.1 "$display" >"$log" 2>&1 &
daemon=$!

tries=0
until [ -S "$socket" ]; do
    if ! kill -0 "$daemon" 2>/dev/null; then
        echo "gtk4-broadwayd exited before creating $socket" >&2
        cat "$log" >&2
        exit 1
    fi
    tries=$((tries + 1))
    if [ "$tries" -gt 50 ]; then
        echo "gtk4-broadwayd did not create $socket within 5s" >&2
        cat "$log" >&2
        exit 1
    fi
    sleep 0.1
done

# COUNTERPOINT_REQUIRE_DISPLAY makes the test binaries fail loudly instead of printing SKIPPED if
# this display turns out not to be usable, instead of silently passing. dbus-run-session gives
# the command its own private session bus, so a headless run can never forward to, or be
# activated by, a Counterpoint instance on the user's own D-Bus session.
COUNTERPOINT_REQUIRE_DISPLAY=1 GDK_BACKEND=broadway BROADWAY_DISPLAY="$display" dbus-run-session -- "$@"
