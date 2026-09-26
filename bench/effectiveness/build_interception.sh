#!/usr/bin/env bash
# Build graphite + graphite-hook from a branch (default feat/interception) into
# work/target-<name>, leaving the repo's own target/ untouched. Prints the two
# env lines arm C needs.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(git -C "$HERE" rev-parse --show-toplevel)"
BRANCH="${1:-feat/interception}"
NAME="${BRANCH//\//-}"
SRC="$HERE/work/src-$NAME"
TARGET="$HERE/work/target-$NAME"

if [ -d "$SRC" ]; then
  git -C "$SRC" checkout -q --detach "$BRANCH"
else
  git -C "$REPO" worktree add -q --detach "$SRC" "$BRANCH"
fi
cargo build --release --manifest-path "$SRC/Cargo.toml" --target-dir "$TARGET" \
  -p graphite-cli -p graphite-hooks
echo "built $(git -C "$SRC" rev-parse --short HEAD) ($BRANCH)"
echo "export GRAPHITE_BIN=$TARGET/release/graphite"
echo "export GRAPHITE_HOOK_BIN=$TARGET/release/graphite-hook"
