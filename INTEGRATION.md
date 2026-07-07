# ReviveL Companion — Integration with ReviveL Browser Extension

## Goal
The companion provides a full-featured local `lbrynet` instance so the browser extension can use real LBC wallet operations (publish with bids, pay for content, send/tip, balance) instead of the public Odysee HTTP proxy.

## Architecture

The ReviveL Companion does **not** implement wallet logic or talk directly to SPV servers.

```
Extension
   │ HTTP JSON-RPC
   │ http://127.0.0.1:5279
   ▼
lbrynet  (the process started by the Companion)
   │ SPV protocol (Electrum-style)
   ▼
Remote SPV servers
   (the ones listed under lbryum_servers in the config)
```

- **Everything the extension does** (including `address_unused`, `wallet_send`, `txo_list`, balances, etc.) goes through **lbrynet's JSON-RPC API**.
- The Companion's only responsibilities are:
  - Starting/stopping/configuring the `lbrynet` process
  - Writing a sane `daemon_settings.yml` (including `lbryum_servers`)
  - Registering the `lbry://` protocol handler at the OS level
  - Exposing a reliable local endpoint with proper CORS (`--allowed-origin "*"`)
- `lbryum` / SPV servers are **only** spoken to by `lbrynet` itself using the SPV protocol.
- The Companion **never** speaks the SPV protocol and **never** sends wallet commands (address_unused, wallet_send, txo_list, etc.) directly to SPV servers. All wallet commands come from the extension → lbrynet's JSON-RPC API.

This is the same architecture the old LBRY Desktop used when running in SPV mode.

## RPC Endpoint
- **URL**: `http://127.0.0.1:5279`
- **Protocol**: JSON-RPC over HTTP POST
- **No authentication** required when connecting from localhost (standard lbrynet behavior).
- Default port is 5279 — the same one used by classic LBRY Desktop.

Example request (status):
```json
POST http://127.0.0.1:5279
Content-Type: application/json

{
  "method": "status",
  "params": {}
}
```

Successful response contains:
- `result.wallet.connected` (SPV server)
- `result.wallet.blocks`, `blocks_behind`
- Many other fields (lbry.tech/api/daemon for full reference)

Other useful methods for the extension:
- `wallet_status`
- `account_list`, `account_balance`
- `address_unused`
- `wallet_send`
- `txo_list` (preferred for history)
- `claim_list`, `publish`
- `send`, `tip`
- `wallet_create`, `wallet_seed`
- `resolve`, `get` (for paid content), etc.

See the detailed wallet/SPV section at the bottom of this document for exact params.

The Companion also provides `lbry://` protocol support (see dedicated section below).

## Detection Strategy (recommended for the extension)

1. **Fast detection** (on popup open or periodically):
   - `fetch('http://127.0.0.1:5279', { method:'POST', body: JSON.stringify({method:'status', params:{}}) })`
   - If response.ok and `result.wallet.connected` is truthy → "Connected to local daemon"

2. **Fallback**:
   - If not reachable or wallet not ready → show "Local daemon not running. Install ReviveL Companion" + download link.

3. **Optional marker file** (future improvement):
   - Companion can write `~/.config/revivel-companion/daemon.json` (or platform equivalent) containing `{ "running": true, "port": 5279, "managed_by": "revivel-companion" }`.
   - Extension can read this via native messaging or a small helper if needed (more complex).

## One-click Download Flow (for extension)

In the extension UI:
- Provide buttons/links to:
  - Latest GitHub Releases page (recommended)
  - Or direct platform installers (once you publish versioned releases)
- Example text: "Download ReviveL Companion for full wallet features"

After install:
- Companion can be configured to start automatically (user choice).
- Extension should poll for a few seconds after user says "I installed it".
- Once the Companion is running, `lbry://` addresses typed in the address bar should also resolve to your player (via the OS protocol handler).

## Startup Flags & Config

The companion launches lbrynet roughly as:
```
lbrynet start \
  --data-dir <app-data>/lbrynet-data \
  --config <app-data>/lbrynet-data/daemon_settings.yml \
  --api 127.0.0.1:5279
```

The generated `daemon_settings.yml` configures **lbrynet** (not the Companion) with:
- `lbryum_servers`: list of public SPV servers that lbrynet will connect to
- `api`: binding address for the JSON-RPC server
- Other resource and behavior defaults

The Companion itself never speaks the SPV protocol. All configuration under `lbryum_servers` is consumed exclusively by the lbrynet process.

Users can edit the yaml inside the "Open Data Folder" for advanced tweaks (e.g. custom SPV servers).

## lbry:// Protocol Support

The Companion registers itself as the OS-level handler for the `lbry:` scheme (Windows registry, macOS Info.plist, Linux .desktop + x-scheme-handler).

When a user types or clicks a `lbry://...` URL in Chrome/Brave:

1. The OS launches the Companion with the URL.
2. The Companion constructs the extension player URL:
   ```
   chrome-extension://<extension-id>/player.html?uri=lbry%3A%2F%2F...
   ```
   (and optionally `&title=...`)
3. It opens that URL in the default browser (which should be Chrome/Brave) and brings its own window forward.

The extension ID used for this is stored in the Companion's settings (editable in the UI) and defaults to the current published ID. It is documented that the ID may change after a Chrome Web Store publish, so the Companion exposes it for users who need to update it.

**No change is required on the extension side for basic functionality** — as long as `player.html?uri=...` works, the flow succeeds. The extension is responsible for focusing an existing tab for the same URI if desired.

The Companion also installs a native messaging host manifest (`revivel_companion`) so the extension can talk to it directly in the future (e.g. for daemon control or sending `open-lbry-uri` messages from the extension to the Companion).

## Distinguishing "Our" Daemon vs Other lbrynet Instances

- Multiple lbrynets on the same machine are possible but will fight over port 5279.
- The Companion uses its own data directory, so the wallet and claims are isolated from a classic LBRY Desktop / Odysee install.
- If port 5279 is already in use when the Companion tries to start lbrynet, the daemon will fail to bind. The Companion UI will show the RPC as not reachable.
- Recommendation to users: close other LBRY/Odysee desktop apps when using the Companion.

## Extension-side Status Messages (suggested)

- "Connected to local daemon (SPV ready)"
- "Daemon running but wallet not synced yet"
- "Daemon not running — click to launch companion"
- "No local daemon detected. Using public proxy (limited features)."

## Uninstall / Cleanup

Companion should offer "Stop + clean shutdown".

On uninstall the platform installer usually removes the app; the data dir (`lbrynet-data`) is left behind intentionally (user wallets/claims).

Users who want a full reset can delete:
- Linux: `~/.local/share/<revivel-companion-id>/lbrynet-data` or `~/.config/revivel-companion`
- macOS: `~/Library/Application Support/com.revivel.companion`
- Windows: `%APPDATA%\com.revivel.companion`

## Future Enhancements (for extension + companion)
- Companion exposes a tiny HTTP status on another port or `/revivel-status` that returns simplified JSON.
- Use a fixed file path that extension can request via chrome.runtime or native messaging.
- Version handshake: extension can call a custom or `version` and verify compatibility.
- Single-instance enforcement.

## References
- lbrynet API: https://lbry.tech/api/daemon (still accurate for v0.113)
- Full method list: run `lbrynet commands` while the daemon is running, or see https://lbry.tech/api/daemon

Keep the RPC contract stable. If you ever change the default port or add auth, update this document and the extension together.

## Wallet / SPV RPC Details (for Extension Builder)

**All wallet commands go through lbrynet.** The Companion never speaks the SPV protocol directly and does not wrap or rename any methods.

The Companion runs **standard lbrynet** (no wrapper around method names). All calls are direct JSON-RPC to http://127.0.0.1:5279.

See the Architecture section above for the full picture.

### Recommended first calls
1. `account_list` → get default `account_id`
2. Use that `account_id` for most wallet calls.

### address_unused (receive address)
```json
{
  "method": "address_unused",
  "params": { "account_id": "optional-account-id" }
}
```
Returns: string (LBRY address, e.g. "b...")

### wallet_send (send LBC / tip)
Sends the **same amount** to one or more addresses.

```json
{
  "method": "wallet_send",
  "params": {
    "amount": "0.001",
    "addresses": ["b...address1", "b...address2"]
  }
}
```

For supporting a claim instead of plain send:
```json
{
  "method": "wallet_send",
  "params": {
    "amount": "0.001",
    "claim_id": "claimid..."
  }
}
```

Returns: transaction result object.

**Note on "addresses map"**: The official SDK uses a **list** for `addresses` (sends the same amount to each). Some clients experiment with a map for per-address amounts, but stick to the list form for compatibility.

### History
Use `txo_list` (preferred over `transaction_list` for most UIs).

```json
{
  "method": "txo_list",
  "params": {
    "type": ["spend", "support", "purchase"],
    "account_id": "optional",
    "page": 1,
    "page_size": 20,
    "resolve": false
  }
}
```

Returns paginated list of txos with `amount`, `txid`, `nout`, `claim_id`, `purchase_receipt`, `height`, etc.

### wallet_create
```json
{
  "method": "wallet_create",
  "params": {
    "wallet_id": "mywallet",
    "password": "optional",
    "seed": "optional-seed-for-restore",
    "create_account": true
  }
}
```

**Important**: Does **not** return the seed. Immediately call `wallet_seed` afterwards to retrieve it.

### wallet_seed
```json
{
  "method": "wallet_seed",
  "params": {
    "wallet_id": "mywallet",
    "password": "if-set"
  }
}
```

Returns:
```json
{ "seed": "word1 word2 word3 ..." }
```

### Delete / Close
- `wallet_remove` (the main delete command):
  ```json
  { "method": "wallet_remove", "params": { "wallet_id": "mywallet" } }
  ```
- There is no standard public `wallet_delete`. Use `wallet_remove`.
- `wallet_close` is internal/not commonly used for user-facing "close wallet".

### Balances
- `wallet_balance` — whole wallet
  ```json
  { "method": "wallet_balance", "params": { "include_claims": false } }
  ```

- `account_balance` — specific account (recommended)
  ```json
  {
    "method": "account_balance",
    "params": {
      "account_id": "from-account-list",
      "include_claims": false
    }
  }
  ```

### account_list (very important)
```json
{ "method": "account_list", "params": {} }
```

Returns list of accounts with their `id`, `name`, `address`, `balance`, etc. Use the default account's `id` for `address_unused`, `account_balance`, `txo_list`, etc.

### wallet_status
```json
{ "method": "wallet_status", "params": {} }
```

Returns wallet sync/connected state (useful alongside the top-level `status`).

## Summary for the Extension
- Always start with `account_list` to get a stable `account_id`.
- Use `address_unused` for receive addresses.
- Use `wallet_send` for sending/tipping.
- Use `txo_list` for transaction history.
- Use `wallet_seed` right after `wallet_create` if you need the mnemonic.
- Use `account_balance` or `wallet_balance` for balances.
- The Companion does **not** rename or wrap these methods — they are direct lbrynet calls.
- For `lbry://` support, the Companion will open `player.html?uri=...` (and optionally `&title=...`). Make sure your player page handles this path robustly. See the "lbry:// Protocol Support" section above.
