#!/bin/bash
set -e

echo "=== ReviveL Companion Complete Multi-Platform Compile ==="
echo "Current version: $(node -p "require('./package.json').version")"
echo "Date: $(date)"
echo

PROJECT_DIR=$(pwd)
BUNDLE_DIR="src-tauri/target/release/bundle"

# Function to clean
clean() {
  echo "Cleaning..."
  cargo clean -p revivel_companion_lib 2>/dev/null || true
  rm -rf "$BUNDLE_DIR" 2>/dev/null || true
}

# 1. Linux native
build_linux() {
  echo "=== Building Linux (native) ==="
  npm run tauri build
  echo "Linux bundles:"
  find "$BUNDLE_DIR" -type f \( -name "*.deb" -o -name "*.rpm" -o -name "*.AppImage" \) | head -5 || echo "No Linux bundles found (may need full build)"
}

# 2. Windows cross
build_windows() {
  echo "=== Building Windows (cross from Linux) ==="
  echo "Using cargo-xwin for MSVC target"
  if command -v cargo-xwin &> /dev/null; then
    cargo xwin build --target x86_64-pc-windows-msvc --release -p revivel_companion_lib || echo "Note: lib build may need xwin setup (run 'cargo xwin' first if fails)"
  else
    echo "cargo-xwin not found, installing..."
    cargo install cargo-xwin --version 0.18.6
    cargo xwin build --target x86_64-pc-windows-msvc --release -p revivel_companion_lib || true
  fi
  # Tauri bundle for Windows target (may produce exe, msi if tools available)
  npm run tauri build -- --target x86_64-pc-windows-msvc || echo "Full Windows bundling limited in cross env - use CI or Windows machine for complete .msi/.exe"
  echo "Windows artifacts (if produced):"
  find src-tauri/target/x86_64-pc-windows-msvc -name "*.exe" -o -name "*.msi" 2>/dev/null | head -5 || echo "No Windows installer artifacts (binary may be in target)"
}

# 3. macOS cross
build_macos() {
  echo "=== Building macOS (cross from Linux) ==="
  echo "Note: Full .app/.dmg with signing requires macOS. Cross compile produces binary only."
  for target in x86_64-apple-darwin aarch64-apple-darwin; do
    echo "Building for $target..."
    npm run tauri build -- --target $target || echo "Cross for $target limited (no full SDK/bundler)"
  done
  # Universal
  npm run tauri build -- --target universal-apple-darwin || echo "Universal macOS cross limited"
  echo "macOS artifacts (binaries only usually):"
  find src-tauri/target -path '*/release/revivel-companion' -type f | grep -E 'darwin' | head -3 || echo "Check target for darwin binaries"
}

# 4. Linux via Docker (reproducible)
build_linux_docker() {
  echo "=== Building Linux via Docker ==="
  if [ -f Dockerfile ]; then
    docker build -t revivel-linux .
    echo "Docker image 'revivel-linux' built."
    # Try export target if defined
    mkdir -p docker-bundles/linux
    if docker build --target export -t revivel-linux-export . --output type=local,dest=docker-bundles/linux 2>/dev/null; then
      echo "Exported Linux bundles to docker-bundles/linux/"
      ls docker-bundles/linux/
    fi
  else
    echo "Dockerfile not found, skipping"
  fi
  if [ -f Dockerfile.windows-cross ]; then
    echo "Building Windows cross Docker image..."
    docker build -t revivel-win-cross -f Dockerfile.windows-cross .
    echo "Windows cross Docker image built (run container to build inside)"
  fi
}

# Main
clean
build_linux
build_windows
build_macos
build_linux_docker

echo
echo "=== All compiles attempted ==="
echo "See BUILD.md and the GitHub workflow for recommended full CI builds on all platforms."
echo "Linux packages should be in $BUNDLE_DIR/"
echo "For complete Windows/macOS packages, trigger the GitHub Actions workflow."
