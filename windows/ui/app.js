// DropLink Desktop UI Client Controller (Steam / Fluent Edition)
(() => {
  // State
  let localDevice = { id: "", name: "This PC", platform: "windows", port: 52520, address: "192.168.1.5" };
  let discoveredPeers = [];
  let stagedFiles = [];
  let currentTransfer = null;
  let historyRecords = [];
  let settings = {
    deviceName: "This PC",
    downloadDir: "C:\\Downloads\\DropLink",
    autoAccept: false,
    autostart: false,
  };

  // DOM Elements
  const tabBtns = document.querySelectorAll(".nav-btn");
  const tabPanes = document.querySelectorAll(".tab-pane");
  const localDeviceNameEl = document.getElementById("local-device-name");
  const btnStatusQr = document.getElementById("btn-status-qr");

  const peersGridEl = document.getElementById("peers-grid");
  const peersCountEl = document.getElementById("peers-count");
  const radarPeersContainer = document.getElementById("radar-peers-container");
  const radarEmptyText = document.getElementById("radar-empty-text");

  const stagedBoxEl = document.getElementById("staged-files-box");
  const stagedCountEl = document.getElementById("staged-count");
  const stagedSizeEl = document.getElementById("staged-size");
  const stagedListEl = document.getElementById("staged-list");
  const btnClearStaged = document.getElementById("btn-clear-staged");

  const btnSelectFiles = document.getElementById("btn-select-files");
  const btnSelectFolder = document.getElementById("btn-select-folder");
  const btnShowQr = document.getElementById("btn-show-qr");

  // Direct IP Connect Elements
  const btnOpenDirectIp = document.getElementById("btn-open-direct-ip");
  const directIpModal = document.getElementById("direct-ip-modal");
  const btnCloseDirectIp = document.getElementById("btn-close-direct-ip");
  const btnSubmitDirectIp = document.getElementById("btn-submit-direct-ip");
  const directIpInput = document.getElementById("direct-ip-input");
  const directIpStatus = document.getElementById("direct-ip-status");

  const historyTbody = document.getElementById("history-tbody");
  const historyEmptyEl = document.getElementById("history-empty");
  const btnClearHistory = document.getElementById("btn-clear-history");

  const settingDeviceNameInput = document.getElementById("setting-device-name");
  const settingDownloadDirInput = document.getElementById("setting-download-dir");
  const btnChangeDownloadDir = document.getElementById("btn-change-download-dir");
  const settingAutoAcceptInput = document.getElementById("setting-auto-accept");
  const settingAutostartInput = document.getElementById("setting-autostart");

  // Floating Transfer Bar
  const transferModal = document.getElementById("transfer-modal");
  const transferFileName = document.getElementById("transfer-file-name");
  const transferMetaText = document.getElementById("transfer-meta-text");
  const transferProgressBar = document.getElementById("transfer-progress-bar");
  const transferEtaText = document.getElementById("transfer-eta-text");
  const btnTransferPause = document.getElementById("btn-transfer-pause");
  const btnTransferStop = document.getElementById("btn-transfer-stop");

  // Incoming Prompt Modal
  const incomingModal = document.getElementById("incoming-modal");
  const incomingSenderName = document.getElementById("incoming-sender-name");
  const incomingSummary = document.getElementById("incoming-summary");
  const incomingSasPin = document.getElementById("incoming-sas-pin");
  const trustSenderCheckbox = document.getElementById("trust-sender-checkbox");
  const btnIncomingAccept = document.getElementById("btn-incoming-accept");
  const btnIncomingDecline = document.getElementById("btn-incoming-decline");

  // QR Code Modal
  const qrModal = document.getElementById("qr-modal");
  const btnCloseQr = document.getElementById("btn-close-qr");
  const qrCanvas = document.getElementById("qr-canvas");
  const qrIpDisplay = document.getElementById("qr-ip-display");
  const qrConnectionString = document.getElementById("qr-connection-string");
  const btnCopyIp = document.getElementById("btn-copy-ip");

  // Format bytes
  function formatBytes(bytes) {
    if (!bytes || bytes <= 0) return "0 B";
    const k = 1024;
    const sizes = ["B", "KB", "MB", "GB", "TB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + " " + sizes[i];
  }

  // Format speed
  function formatSpeed(bps) {
    if (!bps || bps <= 0) return "0.0 MB/s";
    return formatBytes(bps) + "/s";
  }

  // Send message to Rust Host
  function postToHost(cmd, payload = {}) {
    if (window.ipc && window.ipc.postMessage) {
      window.ipc.postMessage(JSON.stringify({ cmd, payload }));
    } else {
      console.log("[IPC Debug]", cmd, payload);
    }
  }

  // Switch Tabs
  tabBtns.forEach(btn => {
    btn.addEventListener("click", () => {
      tabBtns.forEach(b => b.classList.remove("active"));
      tabPanes.forEach(p => p.classList.remove("active"));
      btn.classList.add("active");
      const target = document.getElementById(btn.dataset.tab);
      if (target) target.classList.add("active");
    });
  });

  // Render Discovered Devices (both inside Radar and in Cards List)
  function renderPeers() {
    peersCountEl.textContent = discoveredPeers.length;
    radarPeersContainer.innerHTML = "";
    peersGridEl.innerHTML = "";

    if (discoveredPeers.length === 0) {
      radarEmptyText.style.display = "block";
      return;
    }

    radarEmptyText.style.display = "none";

    discoveredPeers.forEach((peer, idx) => {
      let avatarIcon = "💻";
      if (peer.platform === "ios") avatarIcon = "📱";
      else if (peer.platform === "android") avatarIcon = "🤖";
      else if (peer.platform === "macos") avatarIcon = "🍎";

      // 1. Floating Node inside Radar
      const radarNode = document.createElement("div");
      radarNode.className = "radar-peer-node";
      // Position around radar circle
      const angle = (idx * (360 / Math.max(discoveredPeers.length, 3))) * (Math.PI / 180);
      const radius = 90; // px
      const x = Math.round(Math.cos(angle) * radius);
      const y = Math.round(Math.sin(angle) * radius);
      radarNode.style.left = `calc(50% + ${x}px)`;
      radarNode.style.top = `calc(50% + ${y}px)`;
      radarNode.innerHTML = `
        <div class="radar-node-icon">${avatarIcon}</div>
        <div class="radar-node-name">${peer.name}</div>
      `;
      radarNode.title = `Click to send to ${peer.name} (${peer.address || 'Wi-Fi'})`;
      radarNode.addEventListener("click", () => {
        if (stagedFiles.length > 0) {
          sendStagedFilesTo(peer);
        } else {
          postToHost("select_files");
        }
      });
      radarPeersContainer.appendChild(radarNode);

      // 2. Card in Available Devices Grid
      const card = document.createElement("div");
      card.className = "peer-card";
      card.innerHTML = `
        <div class="peer-avatar">${avatarIcon}</div>
        <div class="peer-info">
          <div class="peer-name" title="${peer.name}">${peer.name}</div>
          <div class="peer-platform-badge">
            <span class="online-dot"></span>
            <span>${peer.platform.toUpperCase()} • ${peer.address || '192.168.1.4'}:${peer.port}</span>
          </div>
        </div>
        <button class="peer-btn-send">${stagedFiles.length > 0 ? "Send Files" : "Send Files"}</button>
      `;

      card.addEventListener("click", () => {
        if (stagedFiles.length > 0) {
          sendStagedFilesTo(peer);
        } else {
          postToHost("select_files");
        }
      });

      peersGridEl.appendChild(card);
    });
  }

  // Render Staged Files
  function renderStagedFiles() {
    if (stagedFiles.length === 0) {
      stagedBoxEl.classList.add("hidden");
      renderPeers();
      return;
    }

    stagedBoxEl.classList.remove("hidden");
    stagedCountEl.textContent = `${stagedFiles.length} file${stagedFiles.length > 1 ? "s" : ""}`;
    const totalBytes = stagedFiles.reduce((acc, f) => acc + (f.size || 0), 0);
    stagedSizeEl.textContent = formatBytes(totalBytes);

    stagedListEl.innerHTML = "";
    stagedFiles.slice(0, 10).forEach(file => {
      const chip = document.createElement("div");
      chip.className = "staged-chip";
      chip.innerHTML = `<span>📄</span> <span class="staged-chip-name" title="${file.name}">${file.name}</span> <span class="staged-chip-size">${formatBytes(file.size || 0)}</span>`;
      stagedListEl.appendChild(chip);
    });

    if (stagedFiles.length > 10) {
      const more = document.createElement("div");
      more.className = "staged-chip";
      more.textContent = `+${stagedFiles.length - 10} more...`;
      stagedListEl.appendChild(more);
    }

    renderPeers();
  }

  // Send Staged Files
  function sendStagedFilesTo(peer) {
    if (stagedFiles.length === 0) return;
    
    // Prevent sending to self
    if (!peer.address || peer.address === "127.0.0.1" || peer.address === localDevice.address) {
      alert("Target device address is invalid or points to this PC. Please enter the iPhone's Wi-Fi IP using 'Connect IP'.");
      return;
    }

    const filePaths = stagedFiles.map(f => f.path);
    postToHost("send_files", {
      peer_id: peer.id,
      host: peer.address,
      port: peer.port || 52520,
      file_paths: filePaths,
    });
  }

  // Render History
  function renderHistory() {
    if (!historyRecords || historyRecords.length === 0) {
      historyEmptyEl.style.display = "block";
      historyTbody.innerHTML = "";
      return;
    }

    historyEmptyEl.style.display = "none";
    historyTbody.innerHTML = "";

    historyRecords.forEach(rec => {
      const tr = document.createElement("tr");
      const directionIcon = rec.direction === "sent" ? "⬆️ Sent" : "⬇️ Received";
      const filesDisplay = rec.file_names.slice(0, 2).join(", ") + (rec.file_names.length > 2 ? ` (+${rec.file_names.length - 2})` : "");
      const timeDisplay = new Date(rec.timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

      tr.innerHTML = `
        <td><strong style="color: #66C0F4;">${directionIcon}</strong></td>
        <td><span title="${rec.file_names.join(', ')}">${filesDisplay}</span></td>
        <td><strong>${rec.peer_name}</strong></td>
        <td>${formatBytes(rec.total_size)}</td>
        <td style="color: #8F98A0">${timeDisplay}</td>
        <td>
          <button class="btn btn-secondary btn-open-folder" style="padding: 4px 10px; font-size: 11px;">Open Folder</button>
        </td>
      `;

      const openBtn = tr.querySelector(".btn-open-folder");
      if (openBtn) {
        openBtn.addEventListener("click", () => {
          postToHost("open_folder", { path: settings.downloadDir });
        });
      }

      historyTbody.appendChild(tr);
    });
  }

  // Button Actions
  btnSelectFiles.addEventListener("click", () => postToHost("select_files"));
  btnSelectFolder.addEventListener("click", () => postToHost("select_folder"));
  btnClearStaged.addEventListener("click", () => {
    stagedFiles = [];
    renderStagedFiles();
  });

  btnClearHistory.addEventListener("click", () => postToHost("clear_history"));
  if (btnChangeDownloadDir) {
    btnChangeDownloadDir.addEventListener("click", () => postToHost("select_download_dir"));
  }

  // Direct IP Connect Modal
  if (btnOpenDirectIp) {
    btnOpenDirectIp.addEventListener("click", () => {
      directIpStatus.textContent = "";
      directIpModal.classList.remove("hidden");
      directIpInput.focus();
    });
  }
  if (btnCloseDirectIp) {
    btnCloseDirectIp.addEventListener("click", () => {
      directIpModal.classList.add("hidden");
    });
  }
  if (btnSubmitDirectIp) {
    btnSubmitDirectIp.addEventListener("click", () => {
      const ip = directIpInput.value.trim();
      if (!ip) return;
      directIpStatus.textContent = `Connecting to ${ip}...`;
      postToHost("direct_connect", { host: ip, port: 52520 });
      setTimeout(() => {
        directIpModal.classList.add("hidden");
      }, 1200);
    });
  }

  // Settings
  settingDeviceNameInput.addEventListener("change", (e) => {
    settings.deviceName = e.target.value;
    updateLocalNameDisplay();
    postToHost("save_settings", settings);
  });
  settingAutoAcceptInput.addEventListener("change", (e) => {
    settings.autoAccept = e.target.checked;
    postToHost("save_settings", settings);
  });
  settingAutostartInput.addEventListener("change", (e) => {
    settings.autostart = e.target.checked;
    postToHost("save_settings", settings);
  });

  // Transfer controls
  btnTransferPause.addEventListener("click", () => postToHost("toggle_pause"));
  btnTransferStop.addEventListener("click", () => postToHost("cancel_transfer"));

  // Incoming transfer responses
  btnIncomingAccept.addEventListener("click", () => {
    incomingModal.classList.add("hidden");
    postToHost("respond_incoming", {
      accepted: true,
      trust: trustSenderCheckbox.checked,
    });
  });
  btnIncomingDecline.addEventListener("click", () => {
    incomingModal.classList.add("hidden");
    postToHost("respond_incoming", {
      accepted: false,
      trust: false,
    });
  });

  // QR Modal
  function openQrModal() {
    renderQrCode();
    qrModal.classList.remove("hidden");
  }

  btnShowQr.addEventListener("click", openQrModal);
  if (btnStatusQr) btnStatusQr.addEventListener("click", openQrModal);
  btnCloseQr.addEventListener("click", () => qrModal.classList.add("hidden"));

  if (btnCopyIp) {
    btnCopyIp.addEventListener("click", () => {
      const ip = localDevice.address || '192.168.1.5';
      const port = localDevice.port || 52520;
      navigator.clipboard.writeText(`${ip}:${port}`);
      btnCopyIp.textContent = "Copied!";
      setTimeout(() => { btnCopyIp.textContent = "Copy"; }, 2000);
    });
  }

  function renderQrCode() {
    const ip = localDevice.address || '192.168.1.5';
    const port = localDevice.port || 52520;
    const connectionStr = `droplink://${ip}:${port}?name=${encodeURIComponent(localDevice.name)}`;
    qrConnectionString.textContent = connectionStr;

    if (qrIpDisplay) {
      qrIpDisplay.textContent = `${ip}:${port}`;
    }

    if (window.DropLinkQR) {
      window.DropLinkQR.draw(connectionStr, qrCanvas, 220);
    }
  }

  function updateLocalNameDisplay() {
    const ipSuffix = localDevice.address ? ` • ${localDevice.address}` : "";
    localDeviceNameEl.textContent = `${settings.deviceName || localDevice.name}${ipSuffix}`;
  }

  // Global Message Handler from Rust Host
  window.__droplink_on_message = function(msg) {
    console.log("[Host -> UI]", msg);
    const { type, data } = msg;

    switch (type) {
      case "init_state":
        localDevice = data.local_device || localDevice;
        settings = data.settings || settings;
        historyRecords = data.history || [];
        discoveredPeers = data.peers || [];
        
        updateLocalNameDisplay();
        settingDeviceNameInput.value = settings.deviceName;
        settingDownloadDirInput.value = settings.downloadDir;
        settingAutoAcceptInput.checked = settings.autoAccept;
        settingAutostartInput.checked = settings.autostart;

        renderPeers();
        renderHistory();
        break;

      case "device_discovered": {
        const idx = discoveredPeers.findIndex(p => 
          p.id === data.id || 
          (p.address && data.address && p.address === data.address) ||
          (p.name === data.name && p.platform === data.platform)
        );
        if (idx >= 0) discoveredPeers[idx] = data;
        else discoveredPeers.push(data);
        renderPeers();
        break;
      }

      case "device_lost":
        discoveredPeers = discoveredPeers.filter(p => p.id !== data);
        renderPeers();
        break;

      case "staged_files":
        stagedFiles = data.files || [];
        renderStagedFiles();
        break;

      case "incoming_prompt": {
        incomingSenderName.textContent = data.sender_name || "Nearby Device";
        incomingSummary.textContent = `${data.file_count || 1} file${(data.file_count || 1) > 1 ? 's' : ''} • ${formatBytes(data.total_size || 0)}`;
        
        const pin = data.sas_pin || "------";
        incomingSasPin.textContent = `${pin.slice(0, 3)}  ${pin.slice(3)}`;
        incomingModal.classList.remove("hidden");
        break;
      }

      case "transfer_progress": {
        transferModal.classList.remove("hidden");
        
        const fileName = data.file_name || data.current_file_name || "Transferring...";
        transferFileName.textContent = fileName;
        
        const transferred = data.bytes_transferred !== undefined ? data.bytes_transferred : (data.total_bytes_transferred || 0);
        const total = data.total_bytes !== undefined ? data.total_bytes : (data.total_bytes_overall || 0);
        const speed = data.speed !== undefined ? data.speed : (data.speed_bytes_per_sec || 0);
        const eta = data.eta_seconds !== undefined ? data.eta_seconds : data.estimated_seconds_remaining;

        const pct = total > 0 ? (transferred / total) * 100 : 0;
        transferProgressBar.style.width = `${pct}%`;

        transferMetaText.textContent = `${formatBytes(transferred)} / ${formatBytes(total)} • ${formatSpeed(speed)}`;
        transferEtaText.textContent = eta ? `~${eta}s remaining` : (pct > 0 ? `${pct.toFixed(0)}%` : "Streaming...");
        break;
      }

      case "transfer_finished":
        transferModal.classList.add("hidden");
        historyRecords = data.history || historyRecords;
        renderHistory();
        break;

      default:
        break;
    }
  };

  // Tell host UI is ready
  postToHost("ui_ready");
})();
