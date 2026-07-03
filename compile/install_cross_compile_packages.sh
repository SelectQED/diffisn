#!/usr/bin/env bash
set -euo pipefail

echo "=== Installing cross-compilation targets ==="

# --- Rust targets ---
echo ""
echo ">>> Adding Rust targets..."
rustup target add x86_64-pc-windows-gnu
rustup target add x86_64-apple-darwin
rustup target add aarch64-apple-darwin

# --- Windows: MinGW-w64 cross-linker ---
echo ""
echo ">>> Installing MinGW-w64 for Windows cross-compilation..."
if command -v apt-get &>/dev/null; then
    sudo apt-get install -y gcc-mingw-w64-x86-64
elif command -v dnf &>/dev/null; then
    sudo dnf install -y mingw64-gcc
elif command -v pacman &>/dev/null; then
    sudo pacman -S --noconfirm mingw-w64-gcc
elif command -v brew &>/dev/null; then
    brew install mingw-w64
else
    echo "WARNING: Could not detect package manager."
    echo "Install mingw-w64 manually for Windows cross-compilation."
fi

# --- macOS: cargo-zigbuild (recommended) ---
echo ""
echo ">>> macOS cross-compilation from Linux requires a Mach-O linker."
echo ">>> The recommended approach is cargo-zigbuild (uses zig as cross-linker):"
echo ""
echo "    cargo install cargo-zigbuild"
echo "    brew install zig   # or: apt install zig, pacman -S zig, etc."
echo ""
echo ">>> Alternative: osxcross (https://github.com/tpoechtrager/osxcross)"
echo "    Requires macOS SDK from Xcode."

echo ""
echo "=== Done ==="
echo "Run scripts/cross_compile.sh to build binaries."
