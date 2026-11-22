#!/usr/bin/env bash
# Pull the latest commit and restart Mrox only if something the live bot uses changed.
# Run by mrox-update.timer; safe to run by hand.
# Live Rust unit (mrox-rust): restarts when rust/ changed and the rebuilt binary differs, or config.yaml changed.
# Live TypeScript unit (mrox): restarts when test.ts, package files or config.yaml changed.
set -euo pipefail
cd "$(dirname "$0")/.."

git fetch --quiet origin
LOCAL=$(git rev-parse HEAD)
REMOTE=$(git rev-parse '@{u}')
[ "$LOCAL" = "$REMOTE" ] && exit 0

echo "Updating $LOCAL -> $REMOTE"
changed() { ! git diff --quiet "$LOCAL" "$REMOTE" -- "$@"; }
git pull --ff-only --quiet

if systemctl is-active --quiet mrox-rust; then
  unit=mrox-rust
  restart=0
  changed config.yaml && restart=1
  if changed rust/; then
    bin=rust/target/release/mrox
    before=$(sha256sum "$bin" 2>/dev/null | cut -d' ' -f1 || true)
    echo "rust/ changed: building the release binary"
    if (cd rust && PATH="$HOME/.cargo/bin:$PATH" nice -n 10 cargo build --release); then
      [ "$(sha256sum "$bin" | cut -d' ' -f1)" != "$before" ] && restart=1
    else
      echo "cargo build failed; keeping the running binary"
    fi
  fi
else
  unit=mrox
  restart=0
  if changed package-lock.json; then
    npm ci --no-audit --no-fund
    restart=1
  fi
  changed test.ts package.json config.yaml && restart=1
fi

if [ "$restart" = 1 ]; then
  sudo -n systemctl restart "$unit"
  echo "Restarted $unit at $(git rev-parse --short HEAD)"
else
  echo "Pulled $(git rev-parse --short HEAD); nothing $unit uses changed, not restarting"
fi
