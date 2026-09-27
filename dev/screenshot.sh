#!/bin/sh
# Regenerates the README screenshots, in GNOME's light and dark style, in a private headless
# mutter, so no window reaches the desktop.
# Usage: dev/screenshot.sh
set -eu

# mutter puts its Wayland socket in XDG_RUNTIME_DIR.
: "${XDG_RUNTIME_DIR:?is not set; export it to a private directory (for example /run/user/\$(id -u)) before running dev/screenshot.sh}"

cd "$(dirname "$0")/.."
cargo build --quiet --example screenshot
mkdir -p docs/screenshots

# The process ID keeps parallel runs from sharing a socket.
wayland_display="counterpoint-screenshot-$$"

shoot() {
    # As in dev/headless.sh: a private session bus, no portals or GVfs, the default settings
    # instead of the user's dconf, and GLib criticals in the app are fatal. The virtual monitor
    # is larger than the window, so the window keeps the size the example asks for.
    GSETTINGS_BACKEND=memory GDK_DEBUG=no-portals ADW_DISABLE_PORTAL=1 GIO_USE_VFS=local \
        dbus-run-session -- \
        mutter --headless --wayland --no-x11 --wayland-display "$wayland_display" \
        --virtual-monitor 1600x1000 -- \
        env -u DISPLAY GDK_BACKEND=wayland WAYLAND_DISPLAY="$wayland_display" GTK_A11Y=none \
        G_DEBUG=fatal-criticals \
        target/debug/examples/screenshot "$@" 2>&1 |
        grep -v '^libmutter-Message\|^\*\* Message' || true
    [ -s "$1" ] || { echo "no screenshot was written to $1" >&2; exit 1; }
}

rm -f docs/screenshots/light.png docs/screenshots/dark.png
shoot docs/screenshots/light.png
shoot docs/screenshots/dark.png --dark
echo "Wrote docs/screenshots/light.png and docs/screenshots/dark.png"
