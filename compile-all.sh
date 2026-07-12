#!/bin/bash
set -e

echo "=== ReviveL Companion - Compile All Platforms ==="
echo "Version: $(grep '"version"' package.json | head -1)"
echo

# Clean previous builds
echo "Cleaning previous builds..."
cargo clean -p revivel_companion_lib || true
rm -rf src-tauri/target/release/bundle || true

# 1. Linux (native)
echo "=== 1. Building Linux (native) ==="
npm run tauri build
echo "Linux bundles:"
ls -l src-tauri/target/release/bundle/{deb,rpm,appimage}/* 2>/dev/null || echo "Check target/release/bundle/"

# 2. Windows cross (using cargo-xwin)
echo
echo "=== 2. Building Windows (cross-compile) ==="
echo "Using cargo-xwin for x86_64-pc-windows-msvc"
cargo xwin build --target x86_64-pc-windows-msvc --release -p revivel_companion_lib || echo "Note: Binary build may require full setup"
# For full bundle (may need additional tools like nsis/wix installed)
npm run tauri build -- --target x86_64-pc-windows-msvc || echo "Full Windows bundle may need Windows env or extra tools"
echo "Windows artifacts (if any):"
ls -l src-tauri/target/x86_64-pc-windows-msvc/release/bundle/* 2>/dev/null || echo "No Windows bundle dir (cross bundling limited)"

# 3. macOS cross
echo
echo "=== 3. Building macOS (cross-compile) ==="
echo "Building for x86_64-apple-darwin and aarch64-apple-darwin"
npm run tauri build -- --target x86_64-apple-darwin || echo "macOS x86 cross may have SDK limitations"
npm run tauri build -- --target aarch64-apple-darwin || echo "macOS arm cross may have SDK limitations"
# Universal
npm run tauri build -- --target universal-apple-darwin || echo "Universal macOS may require proper SDK"
echo "macOS artifacts (if any):"
ls -l src-tauri/target/*/release/bundle/* 2>/dev/null | grep -E 'dmg|app' || echo "No macOS bundle (cross bundling limited without macOS env)"

# 4. Linux via Docker (reproducible)
echo
echo "=== 4. Building Linux via Docker (reproducible) ==="
if [ -f Dockerfile ]; then
  docker build -t revivel-linux .
  echo "Docker Linux image built."
  # Extract if using export target
  if docker build --target export -t revivel-linux-export . --output type=local,dest=./docker-linux-bundles 2>/dev/null; then
    echo "Docker bundles exported to docker-linux-bundles/"
    ls -l docker-linux-bundles/
  fi
else
  echo "No Dockerfile found, skipping Docker Linux build"
fi

echo
echo "=== Compile All Done ==="
echo "Check src-tauri/target/release/bundle/ for Linux"
echo "Check src-tauri/target/x86_64-pc-windows-msvc/release/bundle/ for Windows (if successful)"
echo "For full cross-platform packages, use GitHub Actions workflow (recommended)"
echo "See BUILD.md for details"
