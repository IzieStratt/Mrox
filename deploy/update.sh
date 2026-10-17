#!/usr/bin/env bash
# Pull the latest commit and restart Mrox if anything changed.
# Run by mrox-update.timer; safe to run by hand.
set -euo pipefail
cd "$(dirname "$0")/.."

git fetch --quiet origin
LOCAL=$(git rev-parse HEAD)
REMOTE=$(git rev-parse '@{u}')
[ "$LOCAL" = "$REMOTE" ] && exit 0

echo "Updating $LOCAL -> $REMOTE"
LOCK_BEFORE=$(git rev-parse HEAD:package-lock.json)
git pull --ff-only --quiet
if [ "$LOCK_BEFORE" != "$(git rev-parse HEAD:package-lock.json)" ]; then
  npm ci --no-audit --no-fund
fi
sudo -n systemctl restart mrox
echo "Restarted mrox at $(git rev-parse --short HEAD)"
