# ReviveL Companion

**📥 Download the latest release:** [https://revivel.app/#downloads](https://revivel.app/#downloads)

**ReviveL Daemon Bundle** — a small, reliable desktop companion that runs `lbrynet` (SPV wallet mode) so the ReviveL browser extension can unlock full LBC wallet functionality.

RPC endpoint (for the extension and tools): **http://127.0.0.1:5279**

## Features
- Automatically downloads the correct `lbrynet` binary for your OS on first use
- Manages the daemon process (start / stop / auto-restart)
- Pre-configured for reliable public SPV servers (no `lbcd` full node required)
- Simple tray + window UI with clear status
- Auto-start options
- Designed so the ReviveL extension can detect and use it easily

## Requirements (to build)
- Rust (stable)
- Node.js + npm
- Platform build deps (see below)

## Quick Start (Development)
```bash
cd revivel-companion
npm install
npm run tauri dev
```

In the UI:
- Click **Download / Update lbrynet binary** (first run)
- Start Daemon
- Status should show RPC reachable + connected to SPV server

## Building from Source

### Prerequisites
- Rust toolchain (stable, via [rustup](https://rustup.rs/))
- Node.js 18+ and npm
- Git

### Platform-specific setup

**Linux**
```bash
sudo apt-get update
sudo apt-get install -y \
  build-essential curl wget file \
  pkg-config libglib2.0-dev \
  libwebkit2gtk-4.1-dev \
  libgtk-3-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev patchelf
```

**Windows**
- Visual Studio 2022 Build Tools (or full Visual Studio) with "Desktop development with C++"
- WebView2 Evergreen Runtime (usually pre-installed on modern Windows)

**macOS**
- Xcode Command Line Tools: `xcode-select --install`

### Development
```bash
cd revivel-companion
npm install
npm run tauri dev
```

### Local production build (current OS only)
```bash
npm run tauri build
```

Bundles are placed in `src-tauri/target/release/bundle/`:
- Linux: `.deb`, `.AppImage`, `.rpm`
- Windows: `.exe` (and `.msi` if bundler tools are present)
- macOS: `.app` and `.dmg`

### Building for all platforms locally
Where cross-compilation is possible, use the helper script:
```bash
./compile-all.sh
```

This attempts:
- Native Linux build
- Windows cross-compilation (using `cargo-xwin`)
- macOS cross-compilation (binaries only; full app bundles require a macOS machine)
- Reproducible Linux build via Docker

**Note:** Full signed Windows and macOS installers are most reliably produced via the GitHub Actions CI (see below).

### Recommended: Build via GitHub Actions (all platforms)
The repository includes `.github/workflows/build.yml`. On every push it builds on:
- `ubuntu-latest` → Linux bundles
- `windows-latest` → Windows `.exe` / `.msi`
- `macos-latest` → macOS universal `.app` / `.dmg`

Artifacts are uploaded automatically. You can also trigger it manually from the Actions tab.

See [BUILD.md](BUILD.md) for complete cross-platform details, Docker instructions, troubleshooting, and how to include a pre-bundled `lbrynet` binary.

## How the ReviveL Extension Uses It
See [INTEGRATION.md](INTEGRATION.md) for details.

In short:
- The extension can poll `http://127.0.0.1:5279` with a JSON-RPC `status` call.
- If reachable and wallet reports a connected SPV server, use the local daemon for full features.
- Companion writes its data under the platform app data dir (lbrynet-data subdir).

## lbry:// Protocol Support
The Companion registers itself as the handler for the `lbry:` custom scheme on supported OSes (Windows, macOS, Linux).

- When you type a `lbry://...` address in Chrome or Brave (or click such a link), the OS launches the Companion.
- The Companion then opens the ReviveL extension's player: `chrome-extension://mphijnbejfkmcahhjlchcghmjegoefkf/player.html?uri=lbry%3A%2F%2F...`
- A "Register lbry:// protocol handler" button is available in the UI (registration may require restart, admin rights, or proper installation on some systems).
- For the best experience, install the companion and ensure the ReviveL extension is enabled.

See the prompt below (or INTEGRATION.md) for coordination with the browser extension.

## Default SPV Servers
- a-hub1.odysee.com:50001
- s1.lbry.network:50001

You can edit `daemon_settings.yml` inside the data directory if you want to customize.

## Distribution Recommendations
See [DISTRIBUTION.md](DISTRIBUTION.md).

## License
MIT (same as lbry-sdk components where applicable). Use of lbrynet binaries is per their original license.

## Credits
- lbrynet binaries from https://github.com/lbryio/lbry-sdk (v0.113.0)
- Built with Tauri 2

---

**Note**: This is a minimal launcher for power users of the ReviveL extension. It is not a full LBRY/Odysee media browser.
