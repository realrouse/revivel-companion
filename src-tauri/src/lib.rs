// ReviveL Companion - lbrynet daemon manager (SPV wallet)
// Provides reliable local daemon at 127.0.0.1:5279 for the browser extension.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{
    menu::{MenuBuilder, MenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
    Manager, State, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_deep_link::DeepLinkExt;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::sleep;

const APP_NAME: &str = "ReviveL Companion";
const RPC_URL: &str = "http://127.0.0.1:5279";
const DEFAULT_PORT: u16 = 5279;

// Binary download info per platform (v0.113.0 is the last official release)
const LBRYNET_VERSION: &str = "0.113.0";

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Download failed: {0}")]
    Download(String),
    #[error("Extraction failed: {0}")]
    Zip(String),
    #[error("Process error: {0}")]
    Process(String),
    #[error("RPC error: {0}")]
    Rpc(String),
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub auto_start_daemon: bool,
    pub auto_launch_os: bool,
    #[serde(default = "default_true")]
    pub allow_revivel_extension: bool,
    /// Extension ID used when forwarding lbry:// URLs. Defaults to the official ReviveL ID.
    #[serde(default = "default_extension_id")]
    pub revivel_extension_id: String,
    /// List of SPV servers for lbrynet. User configurable for failover.
    #[serde(default = "default_spv_servers")]
    pub spv_servers: Vec<String>,
    /// RPC credentials for secure access. Generated randomly by Companion.
    #[serde(default)]
    pub rpcuser: String,
    #[serde(default)]
    pub rpcpass: String,
    /// Persisted cumulative transfer stats (MB). Survives restarts.
    #[serde(default)]
    pub stats_download_total_mb: f64,
    #[serde(default)]
    pub stats_upload_total_mb: f64,
}

fn default_extension_id() -> String {
    // Empty means "not yet set by user" -> trigger first-time setup UI
    "".to_string()
}

fn default_spv_servers() -> Vec<String> {
    // Hostnames preferred for UX and long-term (lbrynet resolves them).
    // IPs were previously used as workaround for certain DNS/router issues at startup.
    vec![
        "s1.lbry.network:50001".to_string(),
        "a-hub1.odysee.com:50001".to_string(),
    ]
}

fn generate_rpc_credentials() -> (String, String) {
    use rand::Rng;
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    let user: String = (0..16).map(|_| {
        let idx = rng.gen_range(0..CHARSET.len());
        CHARSET[idx] as char
    }).collect();
    let pass: String = (0..32).map(|_| {
        let idx = rng.gen_range(0..CHARSET.len());
        CHARSET[idx] as char
    }).collect();
    (user, pass)
}

fn default_true() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            auto_start_daemon: true,
            auto_launch_os: false,
            allow_revivel_extension: true,
            revivel_extension_id: default_extension_id(),
            spv_servers: default_spv_servers(),
            rpcuser: String::new(),
            rpcpass: String::new(),
            stats_download_total_mb: 0.0,
            stats_upload_total_mb: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletInfo {
    pub connected: bool,
    pub connected_server: Option<String>,
    pub blocks: Option<u64>,
    pub blocks_behind: Option<u64>,
    pub available_servers: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct BlobStats {
    pub finished_blobs: Option<u64>,
    pub total_downloaded_mb: Option<f64>,
    pub total_uploaded_mb: Option<f64>,
    pub download_bps: Option<f64>,
    pub upload_bps: Option<f64>,
    pub active_connections: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DaemonStatus {
    pub running: bool,
    pub rpc_reachable: bool,
    pub binary_path: Option<String>,
    pub wallet: Option<WalletInfo>,
    pub error: Option<String>,
    pub version: Option<String>,
    pub allowed_origin: Option<String>,
    pub extension_friendly: bool,
    pub extension_rpc_ok: bool,
    // New for external daemon detection
    pub managed: bool,
    pub listening: bool,
    pub other_daemon_detected: bool,
    // Uptime and recovery info
    pub uptime_secs: Option<u64>,
    pub restart_count: u32,
    pub disconnected_count: u32,
    pub recovery_history: Vec<String>,
    // Download / Upload statistics
    pub stats: Option<BlobStats>,
}

#[derive(Debug)]
struct DaemonManager {
    child: Option<Child>,
    binary_path: Option<PathBuf>,
    data_dir: PathBuf,
    config_path: PathBuf,
    logs_dir: PathBuf,
    allowed_origin: Option<String>,
    extension_id: Option<String>,
    should_be_running: bool,
    start_time: Option<std::time::Instant>,
    restart_count: u32,
    disconnected_count: u32,
    failures: u32,
    last_restart: Option<std::time::Instant>,
    recovery_history: Vec<String>,
    rpcuser: Option<String>,
    rpcpass: Option<String>,
    // Running cumulative stats (loaded from settings, updated over time)
    stats_download_total_mb: f64,
    stats_upload_total_mb: f64,
    last_stats_time: Option<std::time::Instant>,
    last_download_bps: f64,
    last_upload_bps: f64,
}

impl DaemonManager {
    fn new(app_data: PathBuf) -> Self {
        let data_dir = app_data.join("lbrynet-data");
        let logs_dir = app_data.join("logs");
        let config_path = data_dir.join("daemon_settings.yml");

        // Ensure dirs
        let _ = std::fs::create_dir_all(&data_dir);
        let _ = std::fs::create_dir_all(&logs_dir);

        Self {
            child: None,
            binary_path: None,
            data_dir,
            config_path,
            logs_dir,
            allowed_origin: None,
            extension_id: None,
            should_be_running: false,
            start_time: None,
            restart_count: 0,
            disconnected_count: 0,
            failures: 0,
            last_restart: None,
            recovery_history: vec![],
            rpcuser: None,
            rpcpass: None,
            stats_download_total_mb: 0.0,
            stats_upload_total_mb: 0.0,
            last_stats_time: None,
            last_download_bps: 0.0,
            last_upload_bps: 0.0,
        }
    }

    fn binary_name() -> &'static str {
        if cfg!(target_os = "windows") {
            "lbrynet.exe"
        } else {
            "lbrynet"
        }
    }

    fn platform_asset() -> &'static str {
        if cfg!(target_os = "windows") {
            "lbrynet-windows.zip"
        } else if cfg!(target_os = "macos") {
            "lbrynet-mac.zip"
        } else {
            "lbrynet-linux.zip"
        }
    }

    fn download_url() -> String {
        format!(
            "https://github.com/lbryio/lbry-sdk/releases/download/v{}/{}",
            LBRYNET_VERSION,
            Self::platform_asset()
        )
    }

    async fn find_or_download_binary(&mut self) -> Result<PathBuf> {
        if let Some(p) = &self.binary_path {
            if p.exists() {
                return Ok(p.clone());
            }
        }

        let app_data = self.data_dir.parent().unwrap_or(&self.data_dir).to_path_buf();
        let bin_dir = app_data.join("lbrynet-bin");
        let _ = std::fs::create_dir_all(&bin_dir);
        let target = bin_dir.join(Self::binary_name());

        if target.exists() {
            // Make executable on Unix
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&target)?.permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&target, perms)?;
            }
            self.binary_path = Some(target.clone());
            return Ok(target);
        }

        // Download
        let url = Self::download_url();
        let bytes = reqwest::get(&url)
            .await
            .map_err(|e| AppError::Download(e.to_string()))?
            .bytes()
            .await
            .map_err(|e| AppError::Download(e.to_string()))?;

        // Extract the binary from zip (robust: do not assume exactly 1 entry; find by name)
        let reader = std::io::Cursor::new(bytes);
        let mut archive =
            zip::ZipArchive::new(reader).map_err(|e| AppError::Zip(e.to_string()))?;

        // Find the index first (to avoid overlapping mutable borrows on archive), then extract.
        let mut chosen_idx: Option<usize> = None;
        for i in 0..archive.len() {
            {
                let f = archive.by_index(i).map_err(|e| AppError::Zip(e.to_string()))?;
                let name = f.name().to_ascii_lowercase();
                if name.ends_with("lbrynet.exe") || name.ends_with("/lbrynet.exe") || name == "lbrynet" || name.ends_with("/lbrynet") {
                    chosen_idx = Some(i);
                    break;
                }
            }
        }
        let idx = chosen_idx.ok_or_else(|| AppError::Zip("could not find lbrynet binary inside archive".into()))?;
        let mut chosen_file = archive.by_index(idx).map_err(|e| AppError::Zip(e.to_string()))?;

        let mut out_file = std::fs::File::create(&target)?;
        std::io::copy(&mut chosen_file, &mut out_file)?;

        // Permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&target)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&target, perms)?;
        }

        self.binary_path = Some(target.clone());
        Ok(target)
    }

    fn write_config(&self, allow_extension: bool, servers: &[String], rpcuser: &str, rpcpass: &str, extension_id: &str) -> Result<()> {
        // Ensure primary SPV (s1 / 88.99.63.52) is first choice
        let mut servers_list: Vec<String> = servers.to_vec();
        servers_list.sort_by_key(|s| if s.contains("88.99.63.52") || s.contains("s1.lbry.network") { 0 } else { 1 });
        let servers_yaml: Vec<String> = servers_list.iter().map(|s| format!("- {}", s)).collect();
        let allowed_line = if allow_extension {
            // Strict origin restriction using the stable extension ID.
            // This is the primary defense because lbrynet does not enforce Basic Auth on localhost.
            // Only requests with matching Origin header are allowed.
            format!("allowed_origin: \"chrome-extension://{}/\"\n", extension_id)
        } else {
            "# allowed_origin disabled\n".to_string()
        };
        // Sanitize data dir for cross-platform YAML (backslashes on Windows break YAML parsing / escapes in download_directory etc.)
        let data_str = self.data_dir.to_string_lossy().replace('\\', "/");

        let mut content = format!(
            r#"# ReviveL Companion generated config for lbrynet
# Uses SPV mode by default (no full node/lbcd required)

api: 127.0.0.1:{port}

rpcuser: {rpcuser}
rpcpass: {rpcpass}

# Prefer reliable public SPV servers
lbryum_servers:
{servers}

{allowed}

# Reasonable defaults for a companion daemon
save_files: false
download_directory: "{data}/downloads"
blob_lru_cache_size: 100
max_connections_per_download: 8
concurrent_blob_downloads: 2
"#, 
            port = DEFAULT_PORT,
            rpcuser = rpcuser,
            rpcpass = rpcpass,
            servers = servers_yaml.join("\n"),
            allowed = allowed_line,
            data = data_str
        );

        // Dedent to ensure clean YAML (no leading whitespace on keys).
        // This fixes cases where lbrynet sees "0 urls in config".
        content = content
            .lines()
            .map(|line| line.trim_start())
            .collect::<Vec<_>>()
            .join("\n");

        std::fs::write(&self.config_path, content)?;
        Ok(())
    }

    async fn start(&mut self, allow_extension: bool, servers: &[String], rpcuser: &str, rpcpass: &str, extension_id: &str) -> Result<()> {
        // Defensive: never start without credentials
        if rpcuser.is_empty() || rpcpass.is_empty() {
            return Err(AppError::Process(
                "Cannot start managed daemon without RPC credentials. Authentication would not be enforced.".into(),
            ));
        }

        // Always ensure previous instance is cleanly stopped for clean start
        if self.child.is_some() || self.is_running() {
            let _ = self.stop().await;
            sleep(Duration::from_millis(500)).await;
        }

        // Detect if the port is occupied by ANY process (probe without auth)
        if self.is_reachable_without_auth().await {
            // Something is listening. Check if it accepts *our* credentials
            if self.is_rpc_reachable().await {
                return Err(AppError::Process(
                    "Our managed lbrynet daemon is already running on 127.0.0.1:5279.".into(),
                ));
            } else {
                return Err(AppError::Process(
                    "Port 5279 is in use by another lbrynet that does not accept the Companion's credentials. Use 'Force kill existing' button first.".into(),
                ));
            }
        }

        let bin = self.find_or_download_binary().await?;
        let effective_id = if extension_id.is_empty() { EXTENSION_ID } else { extension_id };
        self.write_config(allow_extension, servers, rpcuser, rpcpass, effective_id)?;

        // Launch lbrynet start with our data dir and config.
        // We use strict allowed_origin matching the extension ID.
        // lbrynet does not enforce Basic Auth on localhost, so the origin list
        // is the main control preventing arbitrary web pages from accessing the wallet.
        let mut cmd = Command::new(&bin);
        cmd.arg("start")
            .arg("--data-dir")
            .arg(&self.data_dir)
            .arg("--config")
            .arg(&self.config_path)
            .arg("--api")
            .arg(format!("127.0.0.1:{}", DEFAULT_PORT))
            .arg("--rpcuser")
            .arg(rpcuser)
            .arg("--rpcpass")
            .arg(rpcpass);

        // Provide SPV servers via CLI (in addition to config) to ensure they are used.
        // Using IPs to avoid DNS lookup failures.
        for server in servers {
            cmd.arg("--lbryum-server").arg(server);
        }

        if allow_extension {
            let eff_id = if extension_id.is_empty() { EXTENSION_ID } else { extension_id };
            // Strict origin for the specific extension ID.
            cmd.arg("--allowed-origin").arg(format!("chrome-extension://{}/", eff_id));
        }

        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        // On Windows hide console window if possible
        #[cfg(target_os = "windows")]
        {
            #[allow(unused_imports)]
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        // Write start header to log (daily rotation by unix day)
        let day = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() / 86400;
        let log_path = self.logs_dir.join(format!("lbrynet-{}.log", day));
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
            use std::io::Write;
            let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
            let _ = writeln!(f, "\n=== [{}] DAEMON START ATTEMPT ===\n", ts);
        }

        let mut child = cmd.spawn().map_err(|e| AppError::Process(e.to_string()))?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        self.child = Some(child);
        self.should_be_running = true;
        self.start_time = Some(std::time::Instant::now());
        self.restart_count += 1;
        self.failures = 0;
        self.last_restart = Some(std::time::Instant::now());
        let eff_id = if extension_id.is_empty() { EXTENSION_ID } else { extension_id };
        self.allowed_origin = if allow_extension {
            Some(format!("chrome-extension://{}/", eff_id))
        } else { None };
        self.extension_id = if allow_extension {
            Some(eff_id.to_string())
        } else { None };
        self.rpcuser = Some(rpcuser.to_string());
        self.rpcpass = Some(rpcpass.to_string());

        // Wait up to ~10s for the RPC to become available (lbrynet can take time to start)
        for _ in 0..20 {
            if self.is_rpc_reachable().await {
                break;
            }
            sleep(Duration::from_millis(500)).await;
        }

        // Post-start verification that authentication is enforced.
        // We explicitly test that a request WITHOUT the Authorization header is rejected.
        // This guarantees that the Extension (which now sends Basic Auth via Native Messaging creds)
        // is the only thing that can use the wallet.
        sleep(Duration::from_millis(800)).await;
        let mut auth_enforced = false;
        for _ in 0..4 {
            if self.is_rpc_reachable().await {
                let accepts_unauth = self.is_reachable_without_auth().await;
                if accepts_unauth {
                    self.log_action("WARNING: lbrynet accepted unauthenticated request after start!");
                } else {
                    auth_enforced = true;
                    self.log_action("SUCCESS: lbrynet daemon is enforcing RPC authentication (unauth requests rejected).");
                    break;
                }
            }
            sleep(Duration::from_millis(400)).await;
        }
        if !auth_enforced {
            self.log_action("WARNING: Failed to confirm auth enforcement on the new daemon. Wallet calls may not require credentials.");
        }

        // Capture lbrynet logs to file
        if let Some(out) = stdout {
            let logs_dir = self.logs_dir.clone();
            tokio::spawn(async move {
                tail_to_log(out, logs_dir, "stdout").await;
            });
        }
        if let Some(err) = stderr {
            let logs_dir = self.logs_dir.clone();
            tokio::spawn(async move {
                tail_to_log(err, logs_dir, "stderr").await;
            });
        }

        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        // Always try clean RPC stop (this works even for externally started daemons)
        let _ = self.try_rpc_stop().await;

        if let Some(mut child) = self.child.take() {
            // Give time for clean shutdown
            sleep(Duration::from_millis(600)).await;

            // Then kill our child if still alive
            let _ = child.kill().await;
            let _ = child.wait().await;

            // Extra force kill on Windows (the companion .exe may keep child tree otherwise in some cases)
            #[cfg(target_os = "windows")]
            {
                // Best effort: if the binary name is known, taskkill the lbrynet.exe
                if let Some(bp) = &self.binary_path {
                    if let Some(name) = bp.file_name().and_then(|n| n.to_str()) {
                        let _ = tokio::process::Command::new("taskkill")
                            .args(["/F", "/IM", name, "/T"])
                            .output()
                            .await;
                    }
                }
            }
        }
        self.should_be_running = false;
        self.start_time = None;
        self.allowed_origin = None;
        self.extension_id = None;
        self.rpcuser = None;
        self.rpcpass = None;
        Ok(())
    }

    async fn try_rpc_stop(&self) -> Result<()> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(4))
            .build()
            .map_err(|e| AppError::Rpc(e.to_string()))?;

        let body = serde_json::json!({
            "method": "stop",
            "params": {}
        });

        let mut req = client.post(RPC_URL).json(&body);
        if let (Some(u), Some(p)) = (&self.rpcuser, &self.rpcpass) {
            req = req.basic_auth(u, Some(p));
        }
        let origin_id = self.extension_id.as_deref().unwrap_or(EXTENSION_ID);
        req = req.header("Origin", format!("chrome-extension://{}/", origin_id));

        let _resp = req
            .send()
            .await
            .map_err(|e| AppError::Rpc(e.to_string()))?;

        Ok(())
    }

    async fn is_rpc_reachable(&self) -> bool {
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
        {
            Ok(c) => c,
            Err(_) => return false,
        };

        let body = serde_json::json!({
            "method": "status",
            "params": {}
        });

        let mut req = client.post(RPC_URL).json(&body);
        if let (Some(u), Some(p)) = (&self.rpcuser, &self.rpcpass) {
            req = req.basic_auth(u, Some(p));
        }
        let origin_id = self.extension_id.as_deref().unwrap_or(EXTENSION_ID);
        req = req.header("Origin", format!("chrome-extension://{}/", origin_id));

        match req.send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    /// Probe if anything is listening on the RPC port WITHOUT sending credentials.
    /// Used to detect ANY daemon (authenticated or not) occupying the port.
    async fn is_reachable_without_auth(&self) -> bool {
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
        {
            Ok(c) => c,
            Err(_) => return false,
        };

        let body = serde_json::json!({
            "method": "status",
            "params": {}
        });

        // Send WITHOUT basic_auth on purpose
        let req = client.post(RPC_URL).json(&body);

        match req.send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    async fn query_status(&self) -> Option<WalletInfo> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .ok()?;

        let body = serde_json::json!({ "method": "status", "params": {} });

        let mut req = client.post(RPC_URL).json(&body);
        if let (Some(u), Some(p)) = (&self.rpcuser, &self.rpcpass) {
            req = req.basic_auth(u, Some(p));
        }
        let origin_id = self.extension_id.as_deref().unwrap_or(EXTENSION_ID);
        req = req.header("Origin", format!("chrome-extension://{}/", origin_id));

        let resp = req.send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }

        let json: serde_json::Value = resp.json().await.ok()?;
        let wallet = json.get("result")?.get("wallet")?;

        let connected = wallet.get("connected").and_then(|v| v.as_str()).is_some();
        let connected_server = wallet
            .get("connected")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let blocks = wallet.get("blocks").and_then(|v| v.as_u64());
        let blocks_behind = wallet.get("blocks_behind").and_then(|v| v.as_u64());
        let available_servers = wallet.get("available_servers").and_then(|v| v.as_u64());

        Some(WalletInfo {
            connected,
            connected_server,
            blocks,
            blocks_behind,
            available_servers,
        })
    }

    async fn query_blob_stats(&mut self) -> Option<BlobStats> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .ok()?;

        let body = serde_json::json!({ "method": "status", "params": {} });

        let mut req = client.post(RPC_URL).json(&body);
        if let (Some(u), Some(p)) = (&self.rpcuser, &self.rpcpass) {
            req = req.basic_auth(u, Some(p));
        }
        let origin_id = self.extension_id.as_deref().unwrap_or(EXTENSION_ID);
        req = req.header("Origin", format!("chrome-extension://{}/", origin_id));

        let resp = req.send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }

        let json: serde_json::Value = resp.json().await.ok()?;
        let result = json.get("result")?;

        let mut stats = BlobStats::default();

        // blob_manager stats
        if let Some(blob) = result.get("blob_manager") {
            stats.finished_blobs = blob.get("finished_blobs").and_then(|v| v.as_u64());

            if let Some(conns) = blob.get("connections") {
                // Extract current rates: handle both "object of per-conn bps" and direct number
                let mut down: f64 = 0.0;
                let mut up: f64 = 0.0;

                if let Some(v) = conns.get("incoming_bps") {
                    if let Some(obj) = v.as_object() {
                        for (_, val) in obj {
                            if let Some(bps) = val.as_f64() { down += bps; }
                        }
                    } else if let Some(bps) = v.as_f64() {
                        down = bps;
                    }
                }
                if let Some(v) = conns.get("outgoing_bps") {
                    if let Some(obj) = v.as_object() {
                        for (_, val) in obj {
                            if let Some(bps) = val.as_f64() { up += bps; }
                        }
                    } else if let Some(bps) = v.as_f64() {
                        up = bps;
                    }
                }

                stats.download_bps = Some(down);
                stats.upload_bps = Some(up);

                // Accumulate totals using observed rate * delta_time (in MB).
                // We use the rate seen on this poll as the estimate for the interval just ended.
                let now = std::time::Instant::now();
                if let Some(last_t) = self.last_stats_time {
                    let delta_s = now.duration_since(last_t).as_secs_f64();
                    if delta_s > 0.0 {
                        self.stats_download_total_mb += down * delta_s / (1024.0 * 1024.0);
                        self.stats_upload_total_mb += up * delta_s / (1024.0 * 1024.0);
                    }
                }
                self.last_stats_time = Some(now);
                self.last_download_bps = down;
                self.last_upload_bps = up;
            }

            // Always report the running (persisted + accumulated) totals
            stats.total_downloaded_mb = Some(self.stats_download_total_mb);
            stats.total_uploaded_mb = Some(self.stats_upload_total_mb);
        }

        // Try to get active connections count
        if let Some(blob) = result.get("blob_manager") {
            if let Some(conns) = blob.get("connections") {
                let mut count = 0u64;
                if let Some(incoming) = conns.get("incoming_bps").and_then(|v| v.as_object()) {
                    count += incoming.len() as u64;
                }
                if let Some(outgoing) = conns.get("outgoing_bps").and_then(|v| v.as_object()) {
                    count += outgoing.len() as u64;
                }
                stats.active_connections = Some(count);
            }
        }

        Some(stats)
    }

    /// Test if the RPC accepts requests that include an Origin header (as sent by browser extensions).
    /// This simulates what the ReviveL MV3 extension does. Returns true only if no 403.
    async fn is_extension_accessible(&self) -> bool {
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
        {
            Ok(c) => c,
            Err(_) => return false,
        };

        let body = serde_json::json!({
            "method": "status",
            "params": {}
        });

        // Use the configured allowed origin for the header (to match lbrynet's allowed_origin)
        // or the dev token for testing if not set.
        let origin = self.allowed_origin.as_deref().unwrap_or("chrome-extension://revivel-companion");
        let mut req = client
            .post(RPC_URL)
            .header("Origin", origin)
            .json(&body);
        if let (Some(u), Some(p)) = (&self.rpcuser, &self.rpcpass) {
            req = req.basic_auth(u, Some(p));
        }

        match req.send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    fn is_running(&mut self) -> bool {
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.log_action(&format!("Child exited with status: {:?}", status));
                    // Crash forensics: try to capture recent log tail
                    let day = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() / 86400;
                    let log_path = self.logs_dir.join(format!("lbrynet-{}.log", day));
                    if let Ok(content) = std::fs::read_to_string(&log_path) {
                        let lines: Vec<&str> = content.lines().rev().take(20).collect();
                        let tail = lines.into_iter().rev().collect::<Vec<_>>().join("\n");
                        self.log_action(&format!("Last 20 log lines on crash:\n{}", tail));
                    }
                    self.child = None;
                    false
                }
                Ok(None) => true,
                Err(_) => false,
            }
        } else {
            false
        }
    }

    fn get_binary_path(&self) -> Option<String> {
        self.binary_path
            .as_ref()
            .map(|p| p.display().to_string())
    }

    pub fn uptime_secs(&self) -> Option<u64> {
        self.start_time.map(|t| t.elapsed().as_secs())
    }

    fn log_action(&self, msg: &str) {
        let day = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() / 86400;
        let log_path = self.logs_dir.join(format!("lbrynet-{}.log", day));
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
            use std::io::Write;
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let _ = writeln!(f, "[{}] [COMP] {}", ts, msg);
        }
        // Recovery history for UI
        if msg.to_lowercase().contains("restart") || msg.to_lowercase().contains("recover") {
            // Note: since &self, we can't mutate here easily; history updated in maintain calls
        }
    }
}

/// Async task to tail child process output (stdout/stderr) and append to lbrynet.log with timestamps.
async fn tail_to_log<R: tokio::io::AsyncRead + Unpin + Send + 'static>(
    reader: R,
    logs_dir: PathBuf,
    stream: &'static str,
) {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    let day = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() / 86400;
    let log_path = logs_dir.join(format!("lbrynet-{}.log", day));
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let entry = format!("[{}] [{}] {}", ts, stream, line.trim_end_matches(['\r', '\n']));
                let path = log_path.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    use std::io::Write;
                    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                        let _ = writeln!(f, "{}", entry);
                    }
                }).await;
            }
        }
    }
}

impl DaemonManager {
    /// Maintain daemon uptime: restart if process dead, RPC unreachable, or SPV disconnected for too long.
    /// Supports server rotation for SPV failover.
    /// Uses a grace period after start to avoid killing the daemon while it is still initializing SPV connection.
    async fn maintain(&mut self, allow_extension: bool, servers: &[String], rpcuser: &str, rpcpass: &str, extension_id: &str) {
        if !self.should_be_running {
            return;
        }

        let now = std::time::Instant::now();

        // Progressive backoff for restarts
        if let Some(last) = self.last_restart {
            let delay = std::cmp::min(60, self.failures * 5) as u64;
            if now.duration_since(last).as_secs() < delay {
                return;
            }
        }

        let alive = self.is_running();
        if !alive {
            self.failures += 1;
            self.last_restart = Some(now);
            self.log_action("Daemon process not alive, auto-restarting...");
            self.recovery_history.push(format!("{}: process died", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()));
            if let Err(e) = self.start(allow_extension, servers, rpcuser, rpcpass, extension_id).await {
                self.log_action(&format!("Auto-start failed: {}", e));
            }
            return;
        }

        // Grace period after start: give lbrynet time to initialize and connect to SPV (SPV can take 30-60s)
        let grace_period = std::time::Duration::from_secs(60);
        let in_grace = self.start_time.map_or(false, |t| now.duration_since(t) < grace_period);

        let reachable = self.is_rpc_reachable().await;
        if !reachable {
            if !in_grace {
                self.failures += 1;
                self.last_restart = Some(now);
                self.log_action("RPC not reachable, restarting daemon...");
                self.recovery_history.push(format!("{}: RPC unreachable", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()));
                let _ = self.stop().await;
                tokio::time::sleep(Duration::from_millis(300)).await;
                if let Err(e) = self.start(allow_extension, servers, rpcuser, rpcpass, extension_id).await {
                    self.log_action(&format!("Restart after unreachable failed: {}", e));
                }
            }
            return;
        }

        // Only observe SPV/wallet state for UI and logging.
        // Do NOT auto-restart the daemon just because SPV is not connected yet.
        // lbrynet itself retries SPV connections. Restarting it during startup
        // (as seen in logs) prevents it from ever succeeding.
        // We only restart on process death or complete loss of RPC reachability.
        if let Some(w) = self.query_status().await {
            let degraded = w.blocks_behind.map_or(false, |b| b > 100);
            if degraded {
                self.log_action(&format!("Degraded mode: blocks behind {}", w.blocks_behind.unwrap_or(0)));
            }
            if !w.connected || degraded {
                self.disconnected_count += 1;
            } else {
                self.disconnected_count = 0;
            }
        } else {
            self.disconnected_count += 1;
        }
    }
}

// Global shared state
struct AppState {
    manager: Arc<Mutex<DaemonManager>>,
    settings_path: PathBuf,
    settings: Arc<Mutex<AppSettings>>,
    auto_launcher: Mutex<Option<auto_launch::AutoLaunch>>,
    shutdown: Arc<AtomicBool>,
}

#[tauri::command]
async fn get_status(state: State<'_, AppState>) -> Result<DaemonStatus> {
    let mut mgr = state.manager.lock().await;
    let managed = mgr.is_running();  // our managed child process
    let listening = mgr.is_rpc_reachable().await;
    let wallet = if listening {
        mgr.query_status().await
    } else {
        None
    };

    let extension_rpc_ok = if listening { mgr.is_extension_accessible().await } else { false };

    let uptime = mgr.uptime_secs();
    let restarts = mgr.restart_count;
    let disconn = mgr.disconnected_count;
    let history = mgr.recovery_history.iter().rev().take(10).cloned().collect(); // last 10

    let stats = if listening {
        mgr.query_blob_stats().await
    } else {
        None
    };

    // Persist updated cumulative totals (if we got fresh stats)
    if let Some(st) = &stats {
        let mut settings = state.settings.lock().await;
        let mut changed = false;
        if let Some(dl) = st.total_downloaded_mb {
            if dl > settings.stats_download_total_mb {
                settings.stats_download_total_mb = dl;
                changed = true;
            }
        }
        if let Some(ul) = st.total_uploaded_mb {
            if ul > settings.stats_upload_total_mb {
                settings.stats_upload_total_mb = ul;
                changed = true;
            }
        }
        if changed {
            let _ = std::fs::write(&state.settings_path, serde_json::to_string_pretty(&*settings).unwrap_or_default());
        }
    }

    Ok(DaemonStatus {
        running: managed,
        rpc_reachable: listening,
        binary_path: mgr.get_binary_path(),
        wallet,
        error: None,
        version: Some(LBRYNET_VERSION.to_string()),
        allowed_origin: mgr.allowed_origin.clone(),
        extension_friendly: mgr.allowed_origin.as_ref().map_or(false, |o| o.contains("chrome-extension://") || o == "*"),
        extension_rpc_ok,
        managed,
        listening,
        other_daemon_detected: listening && !managed,
        uptime_secs: uptime,
        restart_count: restarts,
        disconnected_count: disconn,
        recovery_history: history,
        stats,
    })
}

#[tauri::command]
async fn start_daemon(state: State<'_, AppState>) -> Result<()> {
    let settings = state.settings.lock().await.clone();
    let allow = settings.allow_revivel_extension;
    let servers = settings.spv_servers;
    let rpcuser = settings.rpcuser.clone();
    let rpcpass = settings.rpcpass.clone();
    let ext_id = settings.revivel_extension_id.clone();
    let mut mgr = state.manager.lock().await;
    mgr.start(allow, &servers, &rpcuser, &rpcpass, &ext_id).await
}

#[tauri::command]
async fn stop_daemon(state: State<'_, AppState>) -> Result<()> {
    let mut mgr = state.manager.lock().await;
    mgr.stop().await
}

#[tauri::command]
async fn restart_daemon(state: State<'_, AppState>) -> Result<()> {
    {
        let mut mgr = state.manager.lock().await;
        mgr.stop().await?;
    }
    sleep(Duration::from_millis(600)).await;
    let settings = state.settings.lock().await.clone();
    let allow = settings.allow_revivel_extension;
    let servers = settings.spv_servers;
    let rpcuser = settings.rpcuser.clone();
    let rpcpass = settings.rpcpass.clone();
    let ext_id = settings.revivel_extension_id.clone();
    let mut mgr = state.manager.lock().await;
    mgr.start(allow, &servers, &rpcuser, &rpcpass, &ext_id).await
}

#[tauri::command]
async fn force_kill_existing_daemon(state: State<'_, AppState>) -> Result<()> {
    // Stop our managed process (this also does RPC stop)
    {
        let mut mgr = state.manager.lock().await;
        let _ = mgr.stop().await;
    }

    // Aggressive system-level kill for any remaining lbrynet on the port
    #[cfg(unix)]
    {
        let _ = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("fuser -k 5279/tcp 2>/dev/null || (lsof -ti:5279 2>/dev/null | xargs -r kill -9 2>/dev/null) || pkill -f lbrynet || true")
            .output()
            .await;
    }
    #[cfg(windows)]
    {
        let _ = tokio::process::Command::new("taskkill")
            .args(["/F", "/IM", "lbrynet.exe", "/T"])
            .output()
            .await;
    }

    sleep(Duration::from_millis(600)).await;
    Ok(())
}

#[tauri::command]
async fn quit_app(state: State<'_, AppState>, app: tauri::AppHandle) -> Result<()> {
    {
        if let Some(shutdown) = app.try_state::<AppState>().map(|s| s.shutdown.clone()) {
            shutdown.store(true, Ordering::Relaxed);
        }
        let mut mgr = state.manager.lock().await;
        let _ = mgr.stop().await;
    }
    // Give a moment for cleanup
    sleep(Duration::from_millis(400)).await;
    app.exit(0);
    // Guarantee process termination (especially important for the built .exe on Windows 10)
    #[cfg(target_os = "windows")]
    std::process::exit(0);
    #[allow(unreachable_code)]
    Ok(())
}

#[tauri::command]
async fn ensure_binary(state: State<'_, AppState>) -> Result<String> {
    let mut mgr = state.manager.lock().await;
    let p = mgr.find_or_download_binary().await?;
    Ok(p.display().to_string())
}

#[tauri::command]
async fn reset_stats(state: State<'_, AppState>) -> Result<()> {
    // Reset in-memory
    {
        let mut mgr = state.manager.lock().await;
        mgr.stats_download_total_mb = 0.0;
        mgr.stats_upload_total_mb = 0.0;
        mgr.last_stats_time = None;
        mgr.last_download_bps = 0.0;
        mgr.last_upload_bps = 0.0;
    }
    // Reset in settings and persist
    {
        let mut settings = state.settings.lock().await;
        settings.stats_download_total_mb = 0.0;
        settings.stats_upload_total_mb = 0.0;
        let _ = std::fs::write(&state.settings_path, serde_json::to_string_pretty(&*settings).unwrap_or_default());
    }
    Ok(())
}

#[tauri::command]
async fn get_settings(state: State<'_, AppState>) -> Result<AppSettings> {
    let s = state.settings.lock().await.clone();
    Ok(s)
}

#[tauri::command]
async fn save_settings(
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<()> {
    // Load current to preserve generated rpc creds if not provided in this save
    let mut current = state.settings.lock().await.clone();
    current.auto_start_daemon = settings.auto_start_daemon;
    current.auto_launch_os = settings.auto_launch_os;
    current.allow_revivel_extension = settings.allow_revivel_extension;
    current.revivel_extension_id = settings.revivel_extension_id;
    current.spv_servers = settings.spv_servers;
    // Preserve runtime stats (they are not sent from UI)
    // Do not overwrite rpcuser/rpcpass if empty in the incoming settings

    // Persist
    let json = serde_json::to_string_pretty(&current).unwrap_or_default();
    std::fs::write(&state.settings_path, json)?;

    // Apply auto-launch
    let launcher = state.auto_launcher.lock().await;
    if let Some(l) = launcher.as_ref() {
        if current.auto_launch_os {
            let _ = l.enable();
        } else {
            let _ = l.disable();
        }
    }

    // Update in-memory
    *state.settings.lock().await = current.clone();

    // Optionally auto-start daemon
    if current.auto_start_daemon {
        let allow = current.allow_revivel_extension;
        let servers = current.spv_servers.clone();
        let rpcuser = current.rpcuser.clone();
        let rpcpass = current.rpcpass.clone();
        let ext_id = current.revivel_extension_id.clone();
        let mut mgr = state.manager.lock().await;
        if !mgr.is_running() {
            let _ = mgr.start(allow, &servers, &rpcuser, &rpcpass, &ext_id).await;
        }
    }

    Ok(())
}

#[tauri::command]
async fn open_folder(app: tauri::AppHandle, state: State<'_, AppState>, which: String) -> Result<()> {
    let target = {
        let mgr = state.manager.lock().await;
        match which.as_str() {
            "logs" => mgr.logs_dir.clone(),
            _ => mgr.data_dir.clone(),
        }
    };
    let _ = std::fs::create_dir_all(&target);

    // Use opener plugin first (cross platform). Pass owned String to satisfy Into<String>.
    let path_str = target.to_string_lossy().to_string();
    let opened = app.opener().open_path(path_str.clone(), None::<String>).is_ok();

    if !opened {
        // Fallbacks for reliability (esp. on Windows 10 where plugin may be silent)
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("explorer").arg(&path_str).spawn();
        }
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg(&path_str).spawn();
        }
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open").arg(&path_str).spawn();
        }
    }
    Ok(())
}

#[tauri::command]
async fn reveal_path(app: tauri::AppHandle, path: String) -> Result<()> {
    let _ = app.opener().open_path(path, None::<String>);
    Ok(())
}

#[tauri::command]
fn register_lbry_protocol(app: tauri::AppHandle) -> Result<()> {
    #[cfg(desktop)]
    {
        app.deep_link()
            .register("lbry")
            .map_err(|e| AppError::Process(format!("Failed to register lbry:// handler: {}", e)))?;
    }
    // Also try to install native messaging host manifest
    if let Err(e) = install_native_messaging_host() {
        eprintln!("Warning: failed to install native messaging host manifest: {}", e);
    }
    Ok(())
}

#[tauri::command]
fn install_native_messaging_manifest() -> Result<()> {
    install_native_messaging_host()
        .map_err(|e| AppError::Process(format!("Failed to install native messaging manifest: {}", e)))
}

/// Install the native messaging host manifest for Chrome and Brave.
/// This allows the extension to connectNative("revivel_companion").
fn install_native_messaging_host() -> std::result::Result<(), String> {
    let exe_path = std::env::current_exe()
        .map_err(|e| format!("failed to get exe path: {}", e))?;

    // Load the configured extension ID from settings.
    let configured_id: String = std::fs::read_to_string(get_app_settings_path())
        .ok()
        .and_then(|s| serde_json::from_str::<AppSettings>(&s).ok())
        .map(|s| s.revivel_extension_id)
        .unwrap_or_default();

    let mut allowed: Vec<String> = KNOWN_EXTENSION_IDS
        .iter()
        .map(|id| format!("chrome-extension://{}/", id))
        .collect();

    if !configured_id.is_empty() {
        let configured_origin = format!("chrome-extension://{}/", configured_id);
        if !allowed.contains(&configured_origin) {
            allowed.push(configured_origin);
        }
    }

    let manifest = serde_json::json!({
        "name": "revivel_companion",
        "description": "ReviveL Companion native messaging host",
        "path": exe_path.to_string_lossy(),
        "type": "stdio",
        "allowed_origins": allowed
    });

    let manifest_json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("failed to serialize manifest: {}", e))?;

    #[cfg(target_os = "macos")]
    {
        use std::fs;
        use std::path::PathBuf;

        let home = dirs::home_dir().ok_or("no home dir")?;

        for browser in &["Google/Chrome", "BraveSoftware/Brave-Browser"] {
            let dir: PathBuf = home.join("Library/Application Support")
                .join(browser)
                .join("NativeMessagingHosts");
            fs::create_dir_all(&dir).ok();
            let manifest_path = dir.join("revivel_companion.json");
            fs::write(&manifest_path, &manifest_json).map_err(|e| e.to_string())?;
        }
    }

    #[cfg(target_os = "linux")]
    {
        use std::fs;
        use std::path::PathBuf;

        let home = dirs::home_dir().ok_or("no home dir")?;

        for browser in &["google-chrome", "BraveSoftware/Brave-Browser"] {
            let dir: PathBuf = home.join(".config")
                .join(browser)
                .join("NativeMessagingHosts");
            fs::create_dir_all(&dir).ok();
            let manifest_path = dir.join("revivel_companion.json");
            fs::write(&manifest_path, &manifest_json).map_err(|e| e.to_string())?;
        }
    }

    #[cfg(target_os = "windows")]
    {
        use std::fs;
        use std::path::PathBuf;

        let local = std::env::var("LOCALAPPDATA")
            .map_err(|_| "no LOCALAPPDATA".to_string())?;
        let local = PathBuf::from(local);

        for browser in &["Google\\Chrome", "BraveSoftware\\Brave-Browser"] {
            let dir = local.join(browser).join("NativeMessagingHosts");
            fs::create_dir_all(&dir).ok();
            let manifest_path = dir.join("revivel_companion.json");
            fs::write(&manifest_path, &manifest_json).map_err(|e| e.to_string())?;

            // Also write registry entry
            // HKEY_CURRENT_USER\Software\Google\Chrome\NativeMessagingHosts\revivel_companion
            // and for Brave
            let reg_key = format!(r"Software\{}\NativeMessagingHosts\revivel_companion", browser.replace("\\\\", "\\"));
            // Use windows-registry or simple reg add via command for simplicity
            // For robustness, try command
            let _ = std::process::Command::new("reg")
                .args([
                    "add",
                    &format!("HKCU\\{}", reg_key),
                    "/ve",
                    "/t",
                    "REG_SZ",
                    "/d",
                    &manifest_path.to_string_lossy(),
                    "/f",
                ])
                .output();
        }
    }

    Ok(())
}

/// Run the process as a Chrome Native Messaging host.
/// This is invoked when Chrome/Brave launches us with the extension origin as arg.
/// It speaks the native messaging protocol (length-prefixed JSON over stdio).
pub fn run_as_native_messaging_host() {
    use std::io::{self, Read, Write};

    let mut stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        // Read message length (4 bytes, little-endian)
        let mut len_buf = [0u8; 4];
        if stdin.read_exact(&mut len_buf).is_err() {
            break;
        }
        let len = u32::from_le_bytes(len_buf) as usize;
        if len == 0 {
            continue;
        }

        let mut msg_buf = vec![0u8; len];
        if stdin.read_exact(&mut msg_buf).is_err() {
            break;
        }

        let msg_str = match String::from_utf8(msg_buf) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let response = match serde_json::from_str::<serde_json::Value>(&msg_str) {
            Ok(msg) => handle_native_message(msg),
            Err(e) => serde_json::json!({ "error": format!("invalid json: {}", e) }),
        };

        let resp_str = serde_json::to_string(&response).unwrap_or_else(|_| "{}".to_string());
        let resp_bytes = resp_str.as_bytes();
        let resp_len = (resp_bytes.len() as u32).to_le_bytes();

        if stdout.write_all(&resp_len).is_err() || stdout.write_all(resp_bytes).is_err() || stdout.flush().is_err() {
            break;
        }
    }
}

fn handle_native_message(msg: serde_json::Value) -> serde_json::Value {
    if let Some(typ) = msg.get("type").and_then(|v| v.as_str()) {
        match typ {
            "open-lbry-uri" => {
                if let Some(uri) = msg.get("uri").and_then(|v| v.as_str()) {
                    let title = msg.get("title").and_then(|v| v.as_str());
                    eprintln!("[native host] received open-lbry-uri: {} title={:?}", uri, title);

                    // In native host mode (launched by browser for messaging), we can still
                    // try to open the player URL using the OS default browser.
                    // This helps if the extension wants the companion to trigger the navigation.
                    let settings_path = get_app_settings_path();
                    let id = std::fs::read_to_string(&settings_path)
                        .ok()
                        .and_then(|s| serde_json::from_str::<AppSettings>(&s).ok())
                        .map(|s| s.revivel_extension_id)
                        .unwrap_or_else(|| EXTENSION_ID.to_string());
                    let player_url = build_player_url(&id, uri, title);
                    let _ = open_url_in_browser(&player_url);

                    return serde_json::json!({ "success": true });
                }
                return serde_json::json!({ "success": false, "error": "missing uri" });
            }
            "get-rpc-credentials" => {
                // Load credentials from settings (works in host mode too)
                let settings_path = get_app_settings_path();
                if let Ok(content) = std::fs::read_to_string(&settings_path) {
                    if let Ok(settings) = serde_json::from_str::<AppSettings>(&content) {
                        if !settings.rpcuser.is_empty() && !settings.rpcpass.is_empty() {
                            return serde_json::json!({
                                "success": true,
                                "user": settings.rpcuser,
                                "pass": settings.rpcpass
                            });
                        }
                    }
                }
                return serde_json::json!({ "success": false, "error": "credentials not available" });
            }
            "rpc_call" => {
                // Proxy request from the (whitelisted via NM manifest) extension.
                // The Companion adds Basic Auth + the correct Origin header.
                let settings_path = get_app_settings_path();
                if let Ok(content) = std::fs::read_to_string(&settings_path) {
                    if let Ok(settings) = serde_json::from_str::<AppSettings>(&content) {
                        if !settings.rpcuser.is_empty() && !settings.rpcpass.is_empty() {
                            let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
                            let params = msg.get("params").cloned().unwrap_or(serde_json::json!({}));
                            let body = serde_json::json!({
                                "method": method,
                                "params": params
                            });

                            // Use blocking client since we are in sync NM loop
                            let client = reqwest::blocking::Client::builder()
                                .timeout(std::time::Duration::from_secs(30))
                                .build()
                                .unwrap_or_else(|_| reqwest::blocking::Client::new());

                            let origin = format!("chrome-extension://{}/", settings.revivel_extension_id);
                            let req = client.post("http://127.0.0.1:5279")
                                .json(&body)
                                .header("Origin", origin)
                                .basic_auth(&settings.rpcuser, Some(&settings.rpcpass));

                            match req.send() {
                                Ok(resp) => {
                                    if let Ok(json) = resp.json::<serde_json::Value>() {
                                        return json;
                                    } else {
                                        return serde_json::json!({ "error": "invalid response from lbrynet" });
                                    }
                                }
                                Err(e) => {
                                    return serde_json::json!({ "error": format!("proxy request failed: {}", e) });
                                }
                            }
                        }
                    }
                }
                return serde_json::json!({ "success": false, "error": "credentials not available for proxy" });
            }
            "ping" => {
                return serde_json::json!({ "success": true, "pong": true });
            }
            _ => {}
        }
    }
    serde_json::json!({ "success": false, "error": "unknown message type" })
}

fn get_app_settings_path() -> PathBuf {
    let mut path = dirs::data_dir().expect("cannot determine data dir");
    path.push("com.revivel.companion");
    path.push("settings.json");
    path
}

/// Very basic cross-platform URL opener for use in native host mode (no Tauri app handle).
fn open_url_in_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn()?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()?;
    }
    Ok(())
}

// Primary stable ReviveL extension ID from the current .crx / packaged build.
// This ID is the same for every user on every computer and OS.
// Used for:
// - allowed_origin in lbrynet (main security for direct access)
// - Origin header sent by Companion proxy
// - player.html URLs
const EXTENSION_ID: &str = "mphijnbejfkmcahhjlchcghmjegoefkf";

// Hardcoded stable ID (decided by dev for published builds) is always allowed,
// in addition to whatever ID the user configures for their (possibly unpacked) extension.
const KNOWN_EXTENSION_IDS: &[&str] = &[
    "mphijnbejfkmcahhjlchcghmjegoefkf", // stable .crx / published ID (always allowed)
    "revivel-companion",                // dev token
];

fn handle_lbry_url(app: &tauri::AppHandle, args: Vec<String>, extension_id: &str) {
    for arg in args {
        if arg.starts_with("lbry:") {
            // Use the helper so title can be supported in the future / from other callers
            let player_url = build_player_url(extension_id, &arg, None);
            // Open in the default browser (will use Chrome/Brave if set as default, or the extension page)
            let _ = app.opener().open_url(&player_url, None::<&str>);
            // Also ensure companion window is visible
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.set_focus();
            }
            break;
        }
    }
}

/// Build the player URL using the (possibly user-configured) extension ID and optional title.
pub fn build_player_url(extension_id: &str, uri: &str, title: Option<&str>) -> String {
    let encoded_uri = urlencoding::encode(uri);
    let mut url = format!(
        "chrome-extension://{}/player.html?uri={}",
        extension_id, encoded_uri
    );
    if let Some(t) = title {
        let encoded_title = urlencoding::encode(t);
        url.push_str(&format!("&title={}", encoded_title));
    }
    url
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            // Load configured ID if possible for forwarded lbry events (use NM-compatible path)
            let id = {
                let sp = get_app_settings_path();
                std::fs::read_to_string(&sp)
                    .ok()
                    .and_then(|s| serde_json::from_str::<AppSettings>(&s).ok())
                    .map(|s| s.revivel_extension_id)
                    .unwrap_or_else(|| EXTENSION_ID.to_string())
            };
            handle_lbry_url(app, argv, &id);
        }))
        .plugin(tauri_plugin_deep_link::init())
        .setup(|app| {
            // Resolve app data dir
            let app_data = app
                .path()
                .app_data_dir()
                .expect("failed to get app data dir");

            let _ = std::fs::create_dir_all(&app_data);

            // Use the exact same settings path as the pure Native Messaging host mode
            // so that credentials and extension ID are visible when Chrome launches the exe.
            let settings_path = get_app_settings_path();
            // Also ensure the com.revivel.companion dir exists
            if let Some(parent) = settings_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }

            // Load or default settings
            let mut loaded_settings: AppSettings = std::fs::read_to_string(&settings_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();

            // Ensure RPC credentials exist (generate random if missing)
            if loaded_settings.rpcuser.is_empty() || loaded_settings.rpcpass.is_empty() {
                let (user, pass) = generate_rpc_credentials();
                loaded_settings.rpcuser = user;
                loaded_settings.rpcpass = pass;
                // Save immediately
                if let Ok(json) = serde_json::to_string_pretty(&loaded_settings) {
                    let _ = std::fs::write(&settings_path, json);
                }
            }

            // Ensure spv_servers has at least the defaults (in case old settings.json had empty)
            if loaded_settings.spv_servers.is_empty() {
                loaded_settings.spv_servers = default_spv_servers();
                if let Ok(json) = serde_json::to_string_pretty(&loaded_settings) {
                    let _ = std::fs::write(&settings_path, json);
                }
            }

            // Ensure primary SPV (s1) is first
            let mut sv = loaded_settings.spv_servers.clone();
            sv.sort_by_key(|s| if s.contains("88.99.63.52") || s.contains("s1.lbry.network") { 0 } else { 1 });
            loaded_settings.spv_servers = sv;

            // Daemon manager
            let manager = Arc::new(Mutex::new(DaemonManager::new(app_data.clone())));
            // Initialize persisted stats from settings (sync-safe init)
            {
                let mgr = manager.clone();
                let dl = loaded_settings.stats_download_total_mb;
                let ul = loaded_settings.stats_upload_total_mb;
                tauri::async_runtime::spawn(async move {
                    let mut m = mgr.lock().await;
                    m.stats_download_total_mb = dl;
                    m.stats_upload_total_mb = ul;
                });
            }

            // Auto launcher setup
            let current_exe = std::env::current_exe().ok();
            let auto = if let Some(exe) = current_exe {
                let launcher = auto_launch::AutoLaunchBuilder::new()
                    .set_app_name(APP_NAME)
                    .set_app_path(exe.to_string_lossy().as_ref())
                    .set_use_launch_agent(true)
                    .build()
                    .ok();
                launcher
            } else {
                None
            };

            let shutdown = Arc::new(AtomicBool::new(false));
            let state = AppState {
                manager: manager.clone(),
                settings_path,
                settings: Arc::new(Mutex::new(loaded_settings.clone())),
                auto_launcher: Mutex::new(auto),
                shutdown: shutdown.clone(),
            };

            app.manage(state);

            // Supervisor task for auto-recovery and uptime
            let app_handle_sup = app.handle().clone();
            let mgr_sup = manager.clone();
            let shutdown_flag = shutdown;
            tauri::async_runtime::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(5));
                let mut last_tick = std::time::Instant::now();
                loop {
                    interval.tick().await;
                    if shutdown_flag.load(Ordering::Relaxed) {
                        break;
                    }
                    let now = std::time::Instant::now();
                    let gap = now.duration_since(last_tick).as_secs();
                    last_tick = now;
                    if let Some(state) = app_handle_sup.try_state::<AppState>() {
                        let settings = state.settings.lock().await.clone();
                        if settings.auto_start_daemon {
                            let allow = settings.allow_revivel_extension;
                            let servers = settings.spv_servers.clone();
                            let rpcuser = settings.rpcuser.clone();
                            let rpcpass = settings.rpcpass.clone();
                            let mut mgr = mgr_sup.lock().await;
                            if gap > 30 {
                                mgr.log_action(&format!("Long gap {}s detected (suspend/resume?), forcing health check", gap));
                            }
                            mgr.maintain(allow, &servers, &rpcuser, &rpcpass, &settings.revivel_extension_id).await;
                        }
                    }
                }
            });

            // Apply autostart if enabled
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // small delay for startup
                sleep(Duration::from_millis(300)).await;
                if let Some(state) = app_handle.try_state::<AppState>() {
                    let settings = state.settings.lock().await.clone();
                    if settings.auto_launch_os {
                        if let Some(l) = state.auto_launcher.lock().await.as_ref() {
                            let _ = l.enable();
                        }
                    }
                    if settings.auto_start_daemon {
                        let allow = settings.allow_revivel_extension;
                        let servers = settings.spv_servers.clone();
                        let rpcuser = settings.rpcuser.clone();
                        let rpcpass = settings.rpcpass.clone();
                        let ext_id = settings.revivel_extension_id.clone();
                        let mut mgr = state.manager.lock().await;
                        let _ = mgr.start(allow, &servers, &rpcuser, &rpcpass, &ext_id).await;
                    }
                }
            });

            // Create tray
            let show_item = MenuItem::with_id(app, "show", "Show ReviveL Companion", true, None::<&str>)?;
            let start_item = MenuItem::with_id(app, "start", "Start Daemon", true, None::<&str>)?;
            let stop_item = MenuItem::with_id(app, "stop", "Stop Daemon", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

            let menu = MenuBuilder::new(app)
                .items(&[&show_item, &start_item, &stop_item])
                .separator()
                .item(&quit_item)
                .build()?;

            // Clone the manager Arc for use in tray callbacks (avoids lifetime issues)
            let manager_for_tray = manager.clone();

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip(APP_NAME)
                .menu(&menu)
                .on_menu_event(move |app, event| {
                    match event.id().as_ref() {
                        "quit" => {
                            let m = manager_for_tray.clone();
                            let app_clone = app.clone();
                            tauri::async_runtime::spawn(async move {
                                if let Some(shutdown) = app_clone.try_state::<AppState>().map(|s| s.shutdown.clone()) {
                                    shutdown.store(true, Ordering::Relaxed);
                                }
                                let mut mgr = m.lock().await;
                                let _ = mgr.stop().await;
                                sleep(Duration::from_millis(500)).await;
                                app_clone.exit(0);
                            });
                        }
                        "show" => {
                            if let Some(win) = app.get_webview_window("main") {
                                let _ = win.show();
                                let _ = win.set_focus();
                            } else {
                                let _ = WebviewWindowBuilder::new(
                                    app,
                                    "main",
                                    WebviewUrl::App("index.html".into()),
                                )
                                .title(APP_NAME)
                                .build();
                            }
                        }
                        "start" => {
                            if let Some(state) = app.try_state::<AppState>() {
                                let m = manager_for_tray.clone();
                                let settings_arc = state.settings.clone(); // Arc<Mutex> clone is cheap
                                tauri::async_runtime::spawn(async move {
                                    let settings = settings_arc.lock().await.clone();
                                    let allow = settings.allow_revivel_extension;
                                    let servers = settings.spv_servers;
                                    let rpcuser = settings.rpcuser.clone();
                                    let rpcpass = settings.rpcpass.clone();
                                    let ext_id = settings.revivel_extension_id.clone();
                                    let mut mgr = m.lock().await;
                                    let _ = mgr.start(allow, &servers, &rpcuser, &rpcpass, &ext_id).await;
                                });
                            }
                        }
                        "stop" => {
                            let m = manager_for_tray.clone();
                            tauri::async_runtime::spawn(async move {
                                let mut mgr = m.lock().await;
                                let _ = mgr.stop().await;
                            });
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(move |tray, event| {
                    if let TrayIconEvent::Click { .. } = event {
                        let app = tray.app_handle();
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                })
                .build(app)?;

            // Create main window if not present (for some bundle cases)
            if app.get_webview_window("main").is_none() {
                let _ = WebviewWindowBuilder::new(
                    app,
                    "main",
                    WebviewUrl::App("index.html".into()),
                )
                .title(APP_NAME)
                .inner_size(720.0, 520.0)
                .min_inner_size(600.0, 400.0)
                .build();
            }

            // Ensure clean shutdown on window close (close button). On Windows the X often requires extra force.
            if let Some(window) = app.get_webview_window("main") {
                let mgr_for_close = manager.clone();
                let app_handle_for_close = window.app_handle().clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let m = mgr_for_close.clone();
                        let app_handle = app_handle_for_close.clone();
                        tauri::async_runtime::spawn(async move {
                            if let Some(shutdown) = app_handle.try_state::<AppState>().map(|s| s.shutdown.clone()) {
                                shutdown.store(true, Ordering::Relaxed);
                            }
                            {
                                let mut mgr = m.lock().await;
                                let _ = mgr.stop().await;
                            }
                            sleep(Duration::from_millis(400)).await;
                            app_handle.exit(0);
                            // Last resort to guarantee termination (some Windows envs keep process after app.exit)
                            #[cfg(target_os = "windows")]
                            std::process::exit(0);
                        });
                    }
                });
            }

            // Handle lbry:// URLs passed on initial launch (e.g. from OS protocol handler)
            let initial_args: Vec<String> = std::env::args().collect();
            // Try to use configured extension ID from settings (use the common NM-compatible path)
            let initial_ext_id = {
                let sp = get_app_settings_path();
                std::fs::read_to_string(&sp)
                    .ok()
                    .and_then(|s| serde_json::from_str::<AppSettings>(&s).ok())
                    .map(|s| s.revivel_extension_id)
                    .unwrap_or_else(|| EXTENSION_ID.to_string())
            };
            handle_lbry_url(app.handle(), initial_args, &initial_ext_id);

            // Register the lbry:// scheme with the OS (best effort, on supported platforms)
            // This makes the OS launch this app when lbry:// links are opened in browsers.
            #[cfg(desktop)]
            {
                if let Err(e) = app.deep_link().register("lbry") {
                    // Non-fatal on some platforms / first run; full registration may require installer or admin rights
                    eprintln!("Failed to register lbry:// protocol handler (may need installer): {}", e);
                }

                // Also ensure native messaging host manifest is installed for the extension
                if let Err(e) = install_native_messaging_host() {
                    eprintln!("Warning installing native messaging host: {}", e);
                }
            }

            // Register additional commands that need app handle
            // (reveal_path is registered via generate handler)

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            start_daemon,
            stop_daemon,
            restart_daemon,
            force_kill_existing_daemon,
            quit_app,
            ensure_binary,
            reset_stats,
            get_settings,
            save_settings,
            open_folder,
            reveal_path,
            register_lbry_protocol,
            install_native_messaging_manifest
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
