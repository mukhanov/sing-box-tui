#!/bin/sh
# Build sing-box-tui (release) and install it into PATH.
#
# Usage:
#   ./install.sh              -> /usr/local/bin/sing-box-tui
#   ./install.sh ~/.local     -> ~/.local/bin/sing-box-tui
#   PREFIX=/opt ./install.sh  -> /opt/bin/sing-box-tui
#
# sudo is used only when the destination directory is not writable.
set -eu

PREFIX="${1:-${PREFIX:-/usr/local}}"
BIN_DIR="$PREFIX/bin"
BIN_NAME="sing-box-tui"
SRC="target/release/$BIN_NAME"

if ! command -v cargo >/dev/null 2>&1; then
    echo "error: cargo not found in PATH" >&2
    exit 1
fi

echo "==> cargo build --release"
cargo build --release

if [ ! -f "$SRC" ]; then
    echo "error: $SRC not found after build" >&2
    exit 1
fi

mkdir -p "$BIN_DIR"
if [ -w "$BIN_DIR" ]; then
    install -m 0755 "$SRC" "$BIN_DIR/$BIN_NAME"
else
    echo "==> $BIN_DIR needs root; using sudo"
    sudo install -m 0755 "$SRC" "$BIN_DIR/$BIN_NAME"
fi

if ! "$BIN_DIR/$BIN_NAME" --help >/dev/null; then
    echo "error: installed binary failed to run" >&2
    exit 1
fi
echo "==> ok: installed $BIN_DIR/$BIN_NAME (responds to --help)"
