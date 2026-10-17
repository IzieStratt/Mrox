#!/usr/bin/env bash
# One-time setup on the server.
# As root: install deploy/mrox.service to /etc/systemd/system, then
#   echo 'gaypeople ALL=(root) NOPASSWD: /usr/bin/systemctl restart mrox' > /etc/sudoers.d/mrox
#   chmod 440 /etc/sudoers.d/mrox; loginctl enable-linger gaypeople
# As gaypeople: run this script (installs the auto-pull timer).
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p ~/.config/systemd/user
cp mrox-update.service mrox-update.timer ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now mrox-update.timer
echo "Installed. Logs: journalctl --user -u mrox-update -f ; bot: journalctl -u mrox -f"
