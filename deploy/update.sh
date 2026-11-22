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
bin=rust/target/release/mrox
stamp="$bin.tree"   # the rust/ tree the binary was last built from
# A failed build or an out-of-band pull leaves the checkout current but the binary stale: rebuild then too.
stale_bin=0
if systemctl is-active --quiet mrox-rust && [ "$(cat "$stamp" 2>/dev/null)" != "$(git rev-parse "$REMOTE:rust")" ]; then
  stale_bin=1
fi
[ "$LOCAL" = "$REMOTE" ] && [ "$stale_bin" = 0 ] && exit 0

echo "Updating $LOCAL -> $REMOTE"
changed() { ! git diff --quiet "$LOCAL" "$REMOTE" -- "$@"; }
[ "$LOCAL" = "$REMOTE" ] || git pull --ff-only --quiet

if systemctl is-active --quiet mrox-rust; then
  unit=mrox-rust
  restart=0
  changed config.yaml && restart=1
  if changed rust/ || [ "$stale_bin" = 1 ]; then
    before=$(sha256sum "$bin" 2>/dev/null | cut -d' ' -f1 || true)
    echo "rust/ changed: building the release binary"
    if (cd rust && PATH="$HOME/.cargo/bin:$PATH" nice -n 10 cargo build --release); then
      git rev-parse HEAD:rust > "$stamp"
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
