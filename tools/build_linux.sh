#!/usr/bin/env bash
# Linux release build. On Linux it builds natively; elsewhere it cross-builds
# with cargo-zigbuild against a glibc floor so the binary runs on older
# distros too. Extra arguments go to cargo (e.g. --features click_gui).
#
#   tools/build_linux.sh            x86_64
#   tools/build_linux.sh --arm      aarch64
set -euo pipefail

cd "$(dirname "$0")/.."

ARCH=x86_64
kept=()
for arg in "$@"; do
  case "$arg" in
    --arm) ARCH=aarch64 ;;
    *) kept+=("$arg") ;;
  esac
done
set -- ${kept[@]+"${kept[@]}"}

TARGET="$ARCH-unknown-linux-gnu"
GLIBC="${GLIBC:-2.28}"
PROFILE=linux
BIN=torch-client

rustup target list --installed | grep -qx "$TARGET" || rustup target add "$TARGET"

if [ "$(uname -s)" = Linux ] && [ "$(uname -m)" = "$ARCH" ]; then
  echo "==> cargo build --profile $PROFILE --target $TARGET $*"
  cargo build --profile "$PROFILE" --target "$TARGET" "$@"
else
  command -v cargo-zigbuild >/dev/null || {
    echo "cross-building needs cargo-zigbuild and zig: cargo install cargo-zigbuild" >&2
    exit 1
  }
  for arg in "$@"; do
    case "$arg" in
      *audio*) echo "note: audio links ALSA, which needs a Linux sysroot to cross-build" >&2 ;;
    esac
  done
  echo "==> cargo zigbuild --profile $PROFILE --target $TARGET.$GLIBC $*"
  cargo zigbuild --profile "$PROFILE" --target "$TARGET.$GLIBC" "$@"
fi

OUT="target/$TARGET/$PROFILE/$BIN"
[ -f "$OUT" ] || { echo "cargo produced no $OUT" >&2; exit 1; }

mkdir -p dist
STAGE="target/linux-stage/$BIN"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp "$OUT" "$STAGE/$BIN"
cp LICENSE README.md "$STAGE/"
ARCHIVE="dist/$BIN-linux-$ARCH.tar.gz"
tar -C "$(dirname "$STAGE")" -czf "$ARCHIVE" "$BIN"
echo "==> $ARCHIVE"
