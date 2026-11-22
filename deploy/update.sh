#!/usr/bin/env bash
# Pull the latest commit and restart Mrox if anything changed.
# Run by mrox-update.timer; safe to run by hand.
# Rebuilds the Rust binary when rust/ changed. Restarts the Rust unit when it is the live bot, otherwise the TypeScript
# unit (mrox), as before the switchover.
set -euo pipefail
cd "$(dirname "$0")/.."

git fetch --quiet origin
LOCAL=$(git rev-parse HEAD)
REMOTE=$(git rev-parse '@{u}')
[ "$LOCAL" = "$REMOTE" ] && exit 0

echo "Updating $LOCAL -> $REMOTE"
LOCK_BEFORE=$(git rev-parse HEAD:package-lock.json)
RUST_CHANGED=0
git diff --quiet "$LOCAL" "$REMOTE" -- rust/ || RUST_CHANGED=1
git pull --ff-only --quiet
if [ "$LOCK_BEFORE" != "$(git rev-parse HEAD:package-lock.json)" ]; then
  npm ci --no-audit --no-fund
fi
# Only build while the Rust unit is the live bot, so the 2 GB box isn't compiling for nothing
if [ "$RUST_CHANGED" = 1 ] && systemctl is-active --quiet mrox-rust; then
  echo "rust/ changed: building the release binary"
  if ! (cd rust && PATH="$HOME/.cargo/bin:$PATH" cargo build --release); then
    echo "cargo build failed; the Rust unit keeps its previous binary"
  fi
fi
if systemctl is-active --quiet mrox-rust; then
  unit=mrox-rust
else
  unit=mrox
fi
sudo -n systemctl restart "$unit"
echo "Restarted $unit at $(git rev-parse --short HEAD)"
