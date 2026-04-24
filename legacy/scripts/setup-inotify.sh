#!/usr/bin/env bash
# setup-inotify.sh — tune Linux inotify limits for the CST-MCP server
#
# The `notify` crate uses inotify on Linux (Debian, Ubuntu, Mint, …).
# The default kernel limits are low (8192 watches per user) and will cause
# the watcher to silently fail when many files are tracked simultaneously.
#
# Run this script once on your development machine (requires sudo).
# The settings are persisted to /etc/sysctl.d/ so they survive reboots.
#
# Usage:
#   chmod +x scripts/setup-inotify.sh
#   sudo ./scripts/setup-inotify.sh

set -euo pipefail

CONF_FILE="/etc/sysctl.d/99-cst-mcp-inotify.conf"

echo "==> Writing inotify limits to ${CONF_FILE}"

sudo tee "${CONF_FILE}" > /dev/null <<'EOF'
# Limits for the CST-MCP server's filesystem watcher (notify / inotify).
#
# max_user_watches  — maximum number of inotify watch descriptors per user.
#                     Each tracked file consumes one descriptor.
# max_user_instances — maximum number of inotify instances per user.
#                     One instance is created by the cst-mcp-server process.
# max_queued_events  — depth of the event queue before events are dropped.
#                     A larger queue prevents missed events under heavy I/O.

fs.inotify.max_user_watches   = 524288
fs.inotify.max_user_instances = 1024
fs.inotify.max_queued_events  = 32768
EOF

echo "==> Applying settings (sysctl -p)"
sudo sysctl -p "${CONF_FILE}"

echo ""
echo "Current inotify limits:"
sysctl fs.inotify.max_user_watches fs.inotify.max_user_instances fs.inotify.max_queued_events

echo ""
echo "Done.  Limits will persist across reboots."
