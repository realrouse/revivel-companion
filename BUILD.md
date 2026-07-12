# Building ReviveL Companion

## Prerequisites

### All platforms
- Rust toolchain (rustup.rs)
- Node.js 18+ and npm
- Git

### Linux (build host)
```bash
sudo apt-get install -y \
  build-essential curl wget file \
  pkg-config libglib2.0-dev \
  libwebkit2gtk-4.1-dev \
  libgtk-3-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev patchelf
```

### macOS
- Xcode + command line tools
- For universal: additional setup

### Windows
- Visual Studio 2022 build tools or MSVC
- WebView2 Evergreen (usually present)

## Development
```bash
cd revivel-companion
npm install
npm run tauri dev
```

## Production build (current platform)
```bash
npm run tauri build
```

Bundles appear under:
`src-tauri/target/release/bundle/`

## Cross-platform notes

The project is configured for **Windows, macOS, and Linux** (`targets: "all"` in tauri.conf.json + multi-platform icons).

**Building locally**
- You can only build for the OS you are currently on.
- Use GitHub Actions (see `.github/workflows/build.yml`) for official Windows + macOS builds.

**Windows**
- Install Visual Studio Build Tools + WebView2.
- `npm run tauri build` will produce `.msi` and `.exe` (nsis).

**macOS**
- Requires Xcode command line tools.
- Builds universal binary (Intel + Apple Silicon) by default.
- For distribution: you should sign and notarize the app.

**Recommended way to ship**
A GitHub Actions workflow is already set up (`.github/workflows/build.yml`) that builds on:
- `ubuntu-latest` → deb, AppImage, rpm
- `windows-latest` → msi, nsis/exe
- `macos-latest` → dmg, app (universal)

It automatically creates draft releases with all platform artifacts.

## Icons
Replace files in `src-tauri/icons/` with proper set (use `tauri icon` or the tauri-icon generator).

Run `npm run tauri icon ./path/to/icon.png` after placing a source png.

## Cleaning
`cargo clean` inside src-tauri or delete target/.

## Version bumps
Edit `src-tauri/tauri.conf.json` (version) and `src-tauri/Cargo.toml`.

## Troubleshooting builds
- Missing webkit/gtk libs → re-run the apt command above.
- "no such file" on unzip → ensure downloaded asset is complete.
- On macOS notarization: you will need Apple Developer certs + `tauri.conf` signing config.

## Including a pre-bundled binary (optional)
For offline-first or smaller first-run friction you can:
1. Download the zips.
2. Extract and place the `lbrynet` / `lbrynet.exe` into `src-tauri/resources/` or platform specific resource folders.
3. Modify the Rust `find_or_download_binary` to first look for a bundled resource using `tauri::utils::resources` or `include_bytes!` (increases installer size by ~20-30 MB per platform).

Current design prefers download to keep installers tiny (<10 MB typical).
