#!/usr/bin/env bash
# Run a debug build in an isolated nested X server (Xephyr) with its own
# D-Bus session and data/config dirs, so it never touches your real
# clipboard, history or running instance.
#
# Usage: [CM_DISPLAY=:5] scripts/dev-run.sh [clipboard-manager args...]
# Screenshot: ffmpeg -loglevel error -y -f x11grab -video_size 900x800 -i :5 -frames:v 1 shot.png
set -euo pipefail
DISP="${CM_DISPLAY:-:5}"
ROOT="${CM_DEV_ROOT:-/tmp/cm-dev}"
mkdir -p "$ROOT"/{data,config,state}
if ! pgrep -f "Xephyr $DISP" >/dev/null; then
  Xephyr "$DISP" -screen 900x800 -ac -br >/dev/null 2>&1 &
  sleep 1
fi
exec env -u WAYLAND_DISPLAY DISPLAY="$DISP" \
  XDG_DATA_HOME="$ROOT/data" XDG_CONFIG_HOME="$ROOT/config" XDG_STATE_HOME="$ROOT/state" \
  RUST_LOG="${RUST_LOG:-debug}" GDK_DEBUG=no-portals GTK_A11Y=none NO_AT_BRIDGE=1 \
  dbus-run-session -- "${CM_BIN:-./target/debug/clipboard-manager}" "$@"
