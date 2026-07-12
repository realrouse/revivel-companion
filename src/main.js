const { invoke } = window.__TAURI__.core;

let shouldAnimateDots = false;

async function refreshStatus() {
  const indicator = document.getElementById('action-indicator');
  const refreshBtn = document.getElementById('btn-refresh');
  if (refreshBtn) refreshBtn.disabled = true;
  if (indicator) indicator.textContent = '⏳ Refreshing...';
  try {
    const s = await invoke('get_status');
    setStatusUI(s);
    updateConflictUI(s);
  } catch (e) {
    setStatusUI({ error: String(e) });
  } finally {
    if (refreshBtn) refreshBtn.disabled = false;
    if (indicator) indicator.textContent = '';
  }
}

function setStatusUI(s) {
  const proc = document.getElementById('proc-status');
  const rpc = document.getElementById('rpc-status');
  const wallet = document.getElementById('wallet-status');
  const spv = document.getElementById('spv-status');
  const blocks = document.getElementById('blocks-status');

  // Stop any previous dot animations
  if (window._dotsTimer) {
    clearInterval(window._dotsTimer);
    window._dotsTimer = null;
  }

  if (s.error) {
    proc.textContent = 'Error';
    proc.className = 'value stopped';
    rpc.textContent = s.error;
    wallet.textContent = '—';
    spv.textContent = '—';
    blocks.textContent = '—';
    return;
  }

  const running = s.running;
  if (s.other_daemon_detected && !s.managed) {
    proc.textContent = 'External (not by us)';
    proc.className = 'value stopped';
  } else {
    proc.textContent = running ? 'Running' : 'Stopped';
    proc.className = 'value ' + (running ? 'running' : 'stopped');
  }

  if (!s.rpc_reachable && s.running) {
    rpc.textContent = 'Not reachable (auto-recovering...)';
    rpc.className = 'value stopped';
  } else {
    rpc.textContent = s.rpc_reachable ? 'Reachable (127.0.0.1:5279)' : 'Not reachable';
    rpc.className = 'value ' + (s.rpc_reachable ? 'running' : 'stopped');
  }

  if (s.wallet) {
    const w = s.wallet;
    if (!w.connected) {
      wallet.textContent = 'Connecting (SPV)...';
      spv.textContent = 'Recovering...';
    } else {
      wallet.textContent = w.connected ? 'Ready' : (w.locked ? 'Locked' : 'Not ready');
      spv.textContent = w.connected_server || '—';
    }
    const b = w.blocks != null ? `${w.blocks} (behind: ${w.blocks_behind || 0})` : '—';
    blocks.textContent = b;
  } else {
    wallet.textContent = '—';
    spv.textContent = '—';
    blocks.textContent = '—';
  }

  // bin info
  const bin = document.getElementById('bin-info');
  let binText = s.binary_path ? `Binary: ${s.binary_path}` : '';
  if (s.uptime_secs != null) {
    const mins = Math.floor(s.uptime_secs / 60);
    binText += ` | Uptime: ${mins}m`;
  }
  if (s.restart_count > 0) {
    binText += ` | Restarts: ${s.restart_count}`;
  }
  if (s.disconnected_count > 0) {
    binText += ` | SPV issues: ${s.disconnected_count}`;
  }
  if (bin) bin.textContent = binText;

  // Recovery history
  const histDiv = document.getElementById('recovery-history');
  if (histDiv && s.recovery_history && s.recovery_history.length) {
    histDiv.innerHTML = '<strong>Recent recoveries:</strong><br>' + s.recovery_history.slice(0,5).map(h => '• ' + h).join('<br>');
  } else if (histDiv) {
    histDiv.textContent = '';
  }

  // Extension compatibility status
  const extDiv = document.getElementById('ext-compat');
  if (extDiv) {
    if (s.running) {
      if (s.extension_friendly || (s.allowed_origin && s.allowed_origin.includes('chrome-extension://'))) {
        extDiv.innerHTML = '<span class="value running">✅ Launched with specific extension Origin (secure)</span>';
      } else {
        extDiv.innerHTML = '<span class="value stopped">⚠️ Not configured for extension Origin.</span>';
      }
      if (s.extension_rpc_ok === false) {
        extDiv.innerHTML += ' <span class="value stopped">(test with Origin header failed)</span>';
      }
    } else {
      extDiv.textContent = '';
    }
  }

  updateConflictUI(s);

  // Apply enabled/disabled states for main action buttons
  applyButtonStates(s);

  // Start animated dots for connecting states (Status/RPC, SPV Server, Blocks)
  startConnectingDots(s, rpc, spv, blocks);

  // Also update statistics when status refreshes
  renderStats(s);
}

function applyButtonStates(s) {
  const startBtn = document.getElementById('btn-start');
  const stopBtn = document.getElementById('btn-stop');
  const restartBtn = document.getElementById('btn-restart');

  const isRunning = !!s.running || !!s.managed;

  if (startBtn) {
    startBtn.disabled = isRunning;
  }
  if (stopBtn) {
    stopBtn.disabled = !isRunning;
  }
  if (restartBtn) {
    restartBtn.disabled = !isRunning;
  }
}

function startConnectingDots(s, rpcEl, spvEl, blocksEl) {
  if (!shouldAnimateDots) {
    if (window._dotsTimer) {
      clearInterval(window._dotsTimer);
      window._dotsTimer = null;
    }
    return;
  }

  const walletEl = document.getElementById('wallet-status');

  const isRpcConnecting = !s.rpc_reachable;
  const isWalletConnecting = !s.wallet || !s.wallet.connected;
  const isSpvConnecting = !s.wallet || !s.wallet.connected;
  const isBlocksConnecting = !s.wallet || !s.wallet.connected;

  if (!isRpcConnecting && !isWalletConnecting && !isSpvConnecting && !isBlocksConnecting) {
    return;
  }

  let dotCount = 0;
  window._dotsTimer = setInterval(() => {
    dotCount = (dotCount + 1) % 4;
    const dots = '.'.repeat(dotCount);

    if (isRpcConnecting && rpcEl) {
      rpcEl.textContent = 'Checking' + dots;
    }
    if (isWalletConnecting && walletEl) {
      walletEl.textContent = 'Connecting' + dots;
    }
    if (isSpvConnecting && spvEl) {
      spvEl.textContent = 'Connecting' + dots;
    }
    if (isBlocksConnecting && blocksEl) {
      blocksEl.textContent = 'Connecting' + dots;
    }
  }, 400);
}

function startConnectingDotsImmediately() {
  const rpcEl = document.getElementById('rpc-status');
  const walletEl = document.getElementById('wallet-status');
  const spvEl = document.getElementById('spv-status');
  const blocksEl = document.getElementById('blocks-status');

  if (rpcEl) rpcEl.textContent = 'Checking';
  if (walletEl) walletEl.textContent = 'Connecting';
  if (spvEl) spvEl.textContent = 'Connecting';
  if (blocksEl) blocksEl.textContent = 'Connecting';

  if (window._dotsTimer) clearInterval(window._dotsTimer);
  let dotCount = 0;
  window._dotsTimer = setInterval(() => {
    dotCount = (dotCount + 1) % 4;
    const dots = '.'.repeat(dotCount);
    if (rpcEl) rpcEl.textContent = 'Checking' + dots;
    if (walletEl) walletEl.textContent = 'Connecting' + dots;
    if (spvEl) spvEl.textContent = 'Connecting' + dots;
    if (blocksEl) blocksEl.textContent = 'Connecting' + dots;
  }, 400);
}

function updateConflictUI(s) {
  const div = document.getElementById('daemon-conflict');
  if (!div) return;
  if (s.other_daemon_detected && !s.managed) {
    div.style.display = 'block';
  } else {
    div.style.display = 'none';
  }
}

async function callAction(name) {
  const indicator = document.getElementById('action-indicator');
  const actionButtons = document.querySelectorAll('.actions button, #daemon-conflict button');

  shouldAnimateDots = true;
  startConnectingDotsImmediately();  // start animating right away for the fields

  if (indicator) indicator.textContent = '⏳ Working...';
  actionButtons.forEach(b => { b.disabled = true; });

  try {
    await invoke(name);
    // Give daemon a moment to settle
    await new Promise(r => setTimeout(r, 700));
    await refreshStatus();
  } catch (e) {
    alert(name + ' failed: ' + e);
  } finally {
    shouldAnimateDots = false;
    if (window._dotsTimer) {
      clearInterval(window._dotsTimer);
      window._dotsTimer = null;
    }
    actionButtons.forEach(b => { b.disabled = false; });
    if (indicator) indicator.textContent = '';
  }
}

async function downloadBinary() {
  const btn = document.getElementById('btn-download');
  btn.disabled = true;
  btn.textContent = 'Downloading...';
  try {
    const res = await invoke('ensure_binary');
    alert('Binary ready: ' + res);
  } catch (e) {
    alert('Download failed: ' + e);
  } finally {
    btn.disabled = false;
    btn.textContent = 'Download / Update lbrynet binary';
    refreshStatus();
  }
}

async function loadSettings() {
  try {
    const cfg = await invoke('get_settings');
    document.getElementById('auto-start').checked = !!cfg.auto_start_daemon;
    document.getElementById('auto-launch-os').checked = !!cfg.auto_launch_os;
    const allowEl = document.getElementById('allow-extension');
    if (allowEl) {
      allowEl.checked = cfg.allow_revivel_extension !== false; // default true
    }
    // Extension ID (for lbry:// forwarding) - shown for advanced users
    const extIdEl = document.getElementById('extension-id');
    if (extIdEl && cfg.revivel_extension_id) {
      extIdEl.value = cfg.revivel_extension_id;
    }
    const spvEl = document.getElementById('spv-servers');
    if (spvEl && cfg.spv_servers) {
      spvEl.value = cfg.spv_servers.join('\n');
    }
  } catch (_) {}
}

async function saveSettings() {
  const autoStart = document.getElementById('auto-start').checked;
  const autoOs = document.getElementById('auto-launch-os').checked;
  const allowEl = document.getElementById('allow-extension');
  const allowExt = allowEl ? allowEl.checked : true;
  const extIdEl = document.getElementById('extension-id');
  const extId = (extIdEl && extIdEl.value.trim()) || 'bgehhgganagafhmkbpgiockhfpgbhebk';
  const spvEl = document.getElementById('spv-servers');
  const spvServers = (spvEl && spvEl.value.trim()) ? spvEl.value.trim().split(/\r?\n/).filter(s => s.trim()) : ['a-hub1.odysee.com:50001', 's1.lbry.network:50001'];
  try {
    await invoke('save_settings', {
      settings: {
        auto_start_daemon: autoStart,
        auto_launch_os: autoOs,
        allow_revivel_extension: allowExt,
        revivel_extension_id: extId,
        spv_servers: spvServers
      }
    });
  } catch (e) { console.error(e); }
}

async function openFolder(which) {
  try {
    await invoke('open_folder', { which });
  } catch (e) { alert('Failed: ' + e); }
}

window.addEventListener('DOMContentLoaded', () => {
  // Wire main buttons
  document.getElementById('btn-start').addEventListener('click', () => callAction('start_daemon'));
  document.getElementById('btn-stop').addEventListener('click', () => callAction('stop_daemon'));
  document.getElementById('btn-restart').addEventListener('click', () => callAction('restart_daemon'));
  document.getElementById('btn-refresh').addEventListener('click', refreshStatus);
  document.getElementById('btn-download').addEventListener('click', downloadBinary);
  document.getElementById('btn-open-data').addEventListener('click', () => openFolder('data'));
  document.getElementById('btn-open-logs').addEventListener('click', () => openFolder('logs'));

  const registerBtn = document.getElementById('btn-register-lbry');
  const registerStatus = document.getElementById('lbry-register-status');
  if (registerBtn) {
    registerBtn.addEventListener('click', async () => {
      if (registerStatus) registerStatus.textContent = 'Registering...';
      try {
        await invoke('register_lbry_protocol');
        if (registerStatus) {
          registerStatus.textContent = 'Registered (may require restart or admin rights on some systems)';
          setTimeout(() => { if (registerStatus) registerStatus.textContent = ''; }, 4000);
        }
      } catch (e) {
        if (registerStatus) registerStatus.textContent = 'Failed: ' + e;
      }
    });
  }

  // Conflict / external daemon buttons
  const forceKillBtn = document.getElementById('btn-force-kill');
  if (forceKillBtn) forceKillBtn.addEventListener('click', async () => {
    const ind = document.getElementById('action-indicator');
    if (ind) ind.textContent = '⏳ Force killing...';
    try {
      await invoke('force_kill_existing_daemon');
      await new Promise(r => setTimeout(r, 600));
      await refreshStatus();
    } catch (e) {
      alert('Force kill failed: ' + e);
    }
    if (ind) ind.textContent = '';
  });

  const startAnywayBtn = document.getElementById('btn-start-anyway');
  if (startAnywayBtn) startAnywayBtn.addEventListener('click', () => callAction('start_daemon'));

  const quitBtn = document.getElementById('btn-quit');
  if (quitBtn) quitBtn.addEventListener('click', async () => {
    try {
      await invoke('quit_app');
    } catch (e) {
      // fallback
      window.close();
    }
  });

  const autoStartEl = document.getElementById('auto-start');
  const autoOsEl = document.getElementById('auto-launch-os');
  const allowExtEl = document.getElementById('allow-extension');
  autoStartEl.addEventListener('change', saveSettings);
  autoOsEl.addEventListener('change', saveSettings);
  if (allowExtEl) allowExtEl.addEventListener('change', saveSettings);

  const extIdEl = document.getElementById('extension-id');
  if (extIdEl) {
    extIdEl.addEventListener('change', saveSettings);
    extIdEl.addEventListener('blur', saveSettings);
  }
  const spvEl = document.getElementById('spv-servers');
  if (spvEl) {
    spvEl.addEventListener('change', saveSettings);
    spvEl.addEventListener('blur', saveSettings);
  }

  // Copy launch command for advanced users / debugging
  const copyBtn = document.getElementById('btn-copy-launch');
  if (copyBtn) {
    copyBtn.addEventListener('click', async () => {
      const cmd = 'lbrynet start --api 127.0.0.1:5279 --allowed-origin "*"';
      try {
        await navigator.clipboard.writeText(cmd);
        const orig = copyBtn.textContent;
        copyBtn.textContent = 'Copied!';
        setTimeout(() => { copyBtn.textContent = orig; }, 1500);
      } catch (_) {
        alert('Command: ' + cmd);
      }
    });
  }

  document.getElementById('link-docs').addEventListener('click', (e) => {
    e.preventDefault();
    // Will be handled by Rust opener or just info
    alert('See README.md in the installed app or GitHub repo for full documentation.');
  });

  // Initial load
  loadSettings();
  // Initial status + conflict detection + auto-start logic
  refreshStatus().then(async () => {
    try {
      const cfg = await invoke('get_settings');
      // Auto-start on open unless another daemon is detected (the warning case)
      if (cfg.auto_start_daemon) {
        // Fetch fresh status to be sure
        const fresh = await invoke('get_status');
        if (!fresh.other_daemon_detected && !fresh.running) {
          shouldAnimateDots = true;
          startConnectingDotsImmediately();
          await invoke('start_daemon');
          await new Promise(r => setTimeout(r, 700));
          await refreshStatus();
          shouldAnimateDots = false;
          if (window._dotsTimer) {
            clearInterval(window._dotsTimer);
            window._dotsTimer = null;
          }
        }
      }
    } catch (e) {
      // non-fatal
      console.error('Auto-start check failed:', e);
    }
  });

  // Poll status every 4s
  setInterval(refreshStatus, 4000);

  // Tab system
  document.querySelectorAll('.tab').forEach(tab => {
    tab.addEventListener('click', () => {
      // Deactivate all
      document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
      document.querySelectorAll('.tab-content').forEach(c => c.classList.remove('active'));

      // Activate clicked
      tab.classList.add('active');
      const target = tab.getAttribute('data-tab');
      const content = document.getElementById(target);
      if (content) content.classList.add('active');
    });
  });

  // Stats refresh button
  const refreshStatsBtn = document.getElementById('btn-refresh-stats');
  if (refreshStatsBtn) {
    refreshStatsBtn.addEventListener('click', refreshStats);
  }

  // Initial stats load
  refreshStats();
});

// New: fetch and display download/upload statistics
async function refreshStats() {
  try {
    const s = await invoke('get_status');
    renderStats(s);
  } catch (e) {
    renderStats({ stats: null });
  }
}

function renderStats(s) {
  const finished = document.getElementById('stats-finished-blobs');
  const downloaded = document.getElementById('stats-downloaded');
  const uploaded = document.getElementById('stats-uploaded');
  const dlSpeed = document.getElementById('stats-download-speed');
  const upSpeed = document.getElementById('stats-upload-speed');
  const conns = document.getElementById('stats-connections');

  if (!s || !s.stats) {
    if (finished) finished.textContent = '—';
    if (downloaded) downloaded.textContent = '—';
    if (uploaded) uploaded.textContent = '—';
    if (dlSpeed) dlSpeed.textContent = '—';
    if (upSpeed) upSpeed.textContent = '—';
    if (conns) conns.textContent = '—';
    return;
  }

  const st = s.stats;

  if (finished) finished.textContent = st.finished_blobs != null ? st.finished_blobs.toLocaleString() : '—';

  if (downloaded) {
    if (st.total_downloaded_mb != null) {
      downloaded.textContent = st.total_downloaded_mb.toFixed(2) + ' MB';
    } else {
      downloaded.textContent = '—';
    }
  }

  if (uploaded) {
    if (st.total_uploaded_mb != null) {
      uploaded.textContent = st.total_uploaded_mb.toFixed(2) + ' MB';
    } else {
      uploaded.textContent = '—';
    }
  }

  if (dlSpeed) {
    if (st.download_bps != null) {
      const mbps = (st.download_bps / 1024 / 1024).toFixed(2);
      dlSpeed.textContent = `${mbps} MB/s`;
    } else {
      dlSpeed.textContent = '—';
    }
  }

  if (upSpeed) {
    if (st.upload_bps != null) {
      const mbps = (st.upload_bps / 1024 / 1024).toFixed(2);
      upSpeed.textContent = `${mbps} MB/s`;
    } else {
      upSpeed.textContent = '—';
    }
  }

  if (conns) {
    conns.textContent = st.active_connections != null ? st.active_connections : '—';
  }
}
