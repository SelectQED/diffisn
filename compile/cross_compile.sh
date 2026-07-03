#!/usr/bin/env bash
set -euo pipefail

ARCHIVE_DIR="target/release/archives"
mkdir -p "$ARCHIVE_DIR"

build_with_zigbuild() {
    local target="$1"
    local suffix="$2"
    echo ">>> Building $target ..."
    cargo zigbuild --release --target "$target"
    local bin="target/$target/release/diffisn$suffix"
    if [ -f "$bin" ]; then
        cp "$bin" "$ARCHIVE_DIR/diffisn-$target$suffix"
        echo "    -> $ARCHIVE_DIR/diffisn-$target$suffix"
    fi
}

build_with_cargo() {
    local target="$1"
    local suffix="$2"
    echo ">>> Building $target ..."
    cargo build --release --target "$target"
    local bin="target/$target/release/diffisn$suffix"
    if [ -f "$bin" ]; then
        cp "$bin" "$ARCHIVE_DIR/diffisn-$target$suffix"
        echo "    -> $ARCHIVE_DIR/diffisn-$target$suffix"
    fi
}

# --- Linux (native) ---
echo "=== Linux ==="
build_with_cargo x86_64-unknown-linux-gnu ""

# --- Windows ---
echo ""
echo "=== Windows ==="
build_with_cargo x86_64-pc-windows-gnu .exe

# --- macOS ---
echo ""
echo "=== macOS ==="
if command -v cargo-zigbuild &>/dev/null; then
    echo "Using cargo-zigbuild for macOS targets."
    build_with_zigbuild x86_64-apple-darwin ""
    build_with_zigbuild aarch64-apple-darwin ""
else
    echo "cargo-zigbuild not found. Install it with: cargo install cargo-zigbuild"
    echo "Also needs zig: install via your package manager or https://ziglang.org/download/"
    echo ""
    echo "Attempting standard cargo build for macOS targets..."
    echo "(This will fail without osxcross SDK — install osxcross or cargo-zigbuild)"
    echo ""
    build_with_cargo x86_64-apple-darwin "" || true
    build_with_cargo aarch64-apple-darwin "" || true
fi

echo ""
echo "=== Done ==="
ls -la "$ARCHIVE_DIR/"
