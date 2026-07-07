# Prompt for ReviveL Extension Builder

You are building/updating the ReviveL browser extension (Manifest V3, Chrome/Brave/Firefox compatible).

The ReviveL Companion (a Tauri desktop app) has just added support for the `lbry:` custom URL scheme at the OS level.

## Goal
When a user types or clicks a `lbry://` URL in Chrome or Brave (or any browser), it should eventually open the ReviveL extension's player with the correct content.

## Current Implementation in Companion (for your reference)
- The Companion registers itself as the handler for the `lbry:` protocol on Windows, macOS, and Linux (using Tauri deep-link + OS-specific mechanisms like registry, Info.plist, .desktop files).
- When the OS launches the Companion with a `lbry://...` argument (or forwards it to a running instance via single-instance plugin):
  - It constructs: `chrome-extension://bgehhgganagafhmkbpgiockhfpgbhebk/player.html?uri=ENCODED_LBRY_URI`
  - It uses the system opener to launch that URL in the user's default browser.
  - It also brings the Companion window to front (optional side effect).

Example from user testing:
chrome-extension://bgehhgganagafhmkbpgiockhfpgbhebk/player.html?uri=lbry%3A%2F%2F%40Chronicles_of_Bod%237%2FOne-old-fat-bike%2C-one-newly-skinny-man%2C-one-new-360-camera-and-the-one-and-only-traditional-Scotland%231&title=...

The extension already supports `?uri=lbry%3A...` via player.html. We want to make this the official launch path for `lbry://` addresses.

## What the Extension Side Needs

1. **Confirm / stabilize the extension ID**
   - Current hardcoded in Companion: `bgehhgganagafhmkbpgiockhfpgbhebk`
   - Make sure this is the stable published ID for the ReviveL extension in the Chrome Web Store.
   - If it ever changes, we need a way to sync (config file, or document it clearly).

2. **Ensure robust handling of direct launches**
   - The player page (or a background/service worker) must handle being opened directly with `?uri=...` even on first load.
   - Parse the `uri` (or `url`) query param, decode it, and load the LBRY content (resolve, play, etc.).
   - If the lbrynet daemon (via Companion) is not running, show a helpful message or auto-prompt to start the Companion.
   - Ideally, focus an existing player tab if one is already open for the same URI.

3. **Optional but highly recommended: Native Messaging support**
   - Define a native messaging host in the extension manifest.
   - Allow the Companion to send messages like:
     ```json
     { "type": "open-lbry-uri", "uri": "lbry://@channel/video" }
     ```
   - The extension can then open/focus the correct player tab without relying on the browser's external URL launch (better UX, avoids extra windows, works cross-browser).
   - Companion side can implement the native host (stdio JSON protocol).
   - This is the cleanest long-term integration.

4. **Manifest / Protocol Handlers (future-proofing)**
   - Consider adding (or preparing for) the `protocol_handlers` key in manifest.json (supported in Firefox, and coming to Chromium behind flags / future versions):
     ```json
     "protocol_handlers": [
       {
         "protocol": "lbry",
         "name": "ReviveL",
         "uriTemplate": "/player.html?uri=%s"
       }
     ]
     ```
   - Also support `web+lbry` as a fallback if needed for `registerProtocolHandler()` from a page.
   - Document the supported launch format.

5. **User-facing features in the extension**
   - Add a setting/toggle: "Enable lbry:// link support" (or "Handle lbry:// addresses in browser").
   - On enable, guide the user to install the Companion if not detected.
   - Detect whether the local daemon (127.0.0.1:5279) is reachable.
   - When a lbry:// is handled, ensure the extension requests the needed permissions / shows the content using the Companion's lbrynet.

6. **Edge cases to handle**
   - URI encoding/decoding (the Companion will percent-encode the full `lbry://...` value).
   - Titles or extra params that may be passed (see the example URL which sometimes includes `&title=...`).
   - The user may type `lbry://` directly in the address bar — the flow goes: Browser → OS → Companion → (opens extension page).
   - Graceful fallback if Companion is not installed.

## Coordination Notes
- The Companion will be the primary registrar for the `lbry:` scheme at the OS level (this is more reliable than pure web/extension registration for address bar usage on all OSes).
- We are starting with the simple "launch chrome-extension:// URL" approach.
- Adding Native Messaging later will make the handoff seamless.
- Please share the final stable extension ID and any preferred launch URL template.

## Testing
- Build the Companion with the lbry protocol changes.
- On the target OS, install/run it.
- In Chrome/Brave, type `lbry://@somechannel/somevideo` in the address bar.
- Expected: Companion launches (or activates), then the extension player opens with the content.
- Also test clicking `lbry://` links from other apps/websites.

If you have questions about the Companion implementation or want to align on Native Messaging host name / message format, let me know.

This feature will make `lbry://` feel native inside Chrome/Brave when the ReviveL stack is installed.

Thanks!