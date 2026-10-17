#!/usr/bin/env bash
# One-time setup on the server, as the user that runs the bot (not root).
# Needs `sudo loginctl enable-linger $USER` once so user services survive logout.
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p ~/.config/systemd/user
cp mrox.service mrox-update.service mrox-update.timer ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now mrox.service mrox-update.timer
echo "Installed. Logs: journalctl --user -u mrox -f"
