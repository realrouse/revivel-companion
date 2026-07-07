# Distribution & Release Recommendations

## Release Channels
- **GitHub Releases** (primary)
  - Attach all platform installers + checksums (SHA256).
  - Use clear naming: `revivel-companion_0.1.0_linux_amd64.deb`, `.AppImage`, `windows_x64.msi`, `macos_universal.dmg`
  - Include `lbrynet` version note in release body.

- **Auto-updater**
  - Tauri has excellent built-in updater support (v2).
  - Enable via `tauri.conf.json` + a simple static JSON endpoint or GitHub releases endpoint.
  - See Tauri updater docs. Recommended for v1+.

## Installer Size Target
Current approach (download binary on first run) keeps the installer small (~5-12 MB).
Users pay the ~25 MB download cost once.

## Code Signing & Notarization
**Critical for user trust and OS warnings:**

- **Windows**: Code sign the .exe / .msi with an EV or OV certificate (e.g. via DigiCert, Sectigo). Use `tauri.conf.json > bundle > windows > signCommand`.
- **macOS**: Apple Developer ID Application certificate + notarization (`tauri build --ci` flow + notarytool). Without it users get scary "damaged app" or Gatekeeper blocks.
- **Linux**: .deb/.AppImage usually don't require signing but GPG signatures on releases are appreciated.

Start without signing for internal testing, but sign before public distribution.

## Hosting the Binaries
lbrynet binaries are fetched from GitHub release assets of lbryio/lbry-sdk.
If that ever goes away, mirror the three zips (or individual platform binaries) to:
- Your own GitHub repo "lbrynet-prebuilts"
- Or a CDN you control
Update the URLs in `src-tauri/src/lib.rs`.

## Update Strategy for lbrynet itself
- Pin a known good version (currently 0.113.0).
- When a community-maintained newer build appears, evaluate, test SPV + wallet functionality, then bump.
- Provide a "Check for lbrynet update" button that re-runs the download logic.

## Auto-start & Permissions
- The companion asks for auto-launch permission via settings.
- On macOS/Linux the auto-launcher uses launchd / systemd user units (via auto-launch crate).
- Users may need to approve "run at login" in OS settings.

## First-run Experience
- Clear "Download lbrynet" button with progress (current UI is basic — improve with a progress event from Rust).
- Show "Connecting to SPV..." state.
- Good error messages ("Failed to bind port 5279 — is another LBRY app running?").

## Uninstaller / Cleanup
Tauri bundles usually include uninstallers on Windows/macOS.
Make sure data dir is **not** auto-deleted (preserve user wallet).

## Recommended GitHub Release Workflow
1. Tag `v0.1.0`
2. GitHub Action matrix builds on ubuntu/mac/windows-latest
3. Uses `tauri-apps/tauri-action`
4. Uploads all bundles + `latest.json` for updater
5. Draft release notes with:
   - "Requires ReviveL extension vX+"
   - SPV servers used
   - Known issues

## Flatpak / AppImage / Homebrew (future)
- Provide an AppImage for easy Linux "download & run".
- Consider a community Flatpak later.
- macOS: Homebrew cask possible after signing.

## Security Considerations
- The app downloads and executes a binary from the internet on first run. 
- In production:
  - Pin exact SHA256 of the expected lbrynet zip/binary and verify after download.
  - Or bundle a known-good binary and only allow updates from signed sources.
- lbrynet itself has no network auth on localhost — this is by design and acceptable because the companion controls the process.

## Current Version Pin
- lbrynet: v0.113.0 (last official from lbryio/lbry-sdk)
- SPV servers chosen for current reachability (2026-07 verified)

Update this file when changing any of the above.
