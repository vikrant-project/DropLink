package com.droplink.app

import android.Manifest
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.core.*
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import com.droplink.app.discovery.AndroidDiscovery
import com.droplink.app.protocol.*
import com.droplink.app.server.AndroidReceiverServer
import com.droplink.app.transfer.AndroidTransferEngine
import com.droplink.app.ui.theme.*
import kotlinx.coroutines.launch
import java.net.Inet4Address
import java.net.NetworkInterface
import java.util.UUID

class MainActivity : ComponentActivity() {

    private lateinit var localDevice: DeviceInfo
    private lateinit var discovery: AndroidDiscovery
    private lateinit var transferEngine: AndroidTransferEngine
    private var receiverServer: AndroidReceiverServer? = null

    private val stagedUris = mutableStateListOf<Uri>()
    private var incomingPrompt by mutableStateOf<TransferManifest?>(null)
    private var incomingPromptCallback: ((Boolean) -> Unit)? = null
    private var receivingProgress by mutableStateOf<LiveTransferProgress?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()

        val deviceName = "${Build.MANUFACTURER.replaceFirstChar { it.uppercase() }} ${Build.MODEL}"
        val realIp = getLocalWifiIp()

        val prefs = getSharedPreferences("droplink_prefs", MODE_PRIVATE)
        var persistentId = prefs.getString("device_id", null)
        if (persistentId == null) {
            persistentId = UUID.randomUUID().toString()
            prefs.edit().putString("device_id", persistentId).apply()
        }

        localDevice = DeviceInfo(
            id = persistentId,
            name = deviceName,
            platform = Platform.ANDROID,
            version = "1.0.0",
            port = 52520,
            fingerprint = persistentId.replace("-", "").uppercase(),
            address = realIp
        )

        discovery = AndroidDiscovery(this, localDevice)
        transferEngine = AndroidTransferEngine(this, localDevice)
        discovery.start()

        receiverServer = AndroidReceiverServer(
            context = this,
            localDevice = localDevice,
            onIncomingTransfer = { manifest, callback ->
                incomingPrompt = manifest
                incomingPromptCallback = callback
            },
            onReceiveProgress = { name, pct, written, total ->
                if (pct >= 1.0f) {
                    receivingProgress = null
                    runOnUiThread {
                        Toast.makeText(this@MainActivity, "Received $name in Downloads/DropLink", Toast.LENGTH_SHORT).show()
                    }
                } else {
                    receivingProgress = LiveTransferProgress(
                        sessionId = UUID.randomUUID().toString(),
                        status = TransferStatus.TRANSFERRING,
                        currentFileName = name,
                        currentFileIndex = 0,
                        totalFiles = 1,
                        transferredBytes = written,
                        totalBytes = total,
                        speedBytesPerSec = 0.0,
                        estimatedSecondsRemaining = null
                    )
                }
            }
        )
        receiverServer?.start()

        handleShareIntent(intent)

        setContent {
            DropLinkTheme {
                MainScreen()
            }
        }
    }

    private fun getLocalWifiIp(): String {
        try {
            val interfaces = NetworkInterface.getNetworkInterfaces()
            for (intf in interfaces) {
                if (intf.isLoopback || !intf.isUp) continue
                for (addr in intf.inetAddresses) {
                    if (!addr.isLoopbackAddress && addr is Inet4Address) {
                        val ip = addr.hostAddress
                        if (ip != null && !ip.startsWith("127.")) {
                            return ip
                        }
                    }
                }
            }
        } catch (_: Exception) {}
        return "127.0.0.1"
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleShareIntent(intent)
    }

    private fun handleShareIntent(intent: Intent?) {
        if (intent == null) return
        when (intent.action) {
            Intent.ACTION_SEND -> {
                (intent.getParcelableExtra<Uri>(Intent.EXTRA_STREAM))?.let { uri ->
                    stagedUris.add(uri)
                }
            }
            Intent.ACTION_SEND_MULTIPLE -> {
                intent.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM)?.let { list ->
                    stagedUris.addAll(list)
                }
            }
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        discovery.stop()
        receiverServer?.stop()
    }

    @OptIn(ExperimentalMaterial3Api::class)
    @Composable
    fun MainScreen() {
        val peers by discovery.peersFlow.collectAsState()
        val activeTransfer by transferEngine.progressFlow.collectAsState()
        val scope = rememberCoroutineScope()

        var selectedTab by remember { mutableIntStateOf(0) }
        var showDirectIpDialog by remember { mutableStateOf(false) }

        val filePickerLauncher = rememberLauncherForActivityResult(
            contract = ActivityResultContracts.OpenMultipleDocuments()
        ) { uris ->
            if (uris.isNotEmpty()) {
                stagedUris.clear()
                stagedUris.addAll(uris)
            }
        }

        val permissionLauncher = rememberLauncherForActivityResult(
            contract = ActivityResultContracts.RequestMultiplePermissions()
        ) { /* permissions handled */ }

        LaunchedEffect(Unit) {
            val perms = mutableListOf(Manifest.permission.INTERNET, Manifest.permission.ACCESS_WIFI_STATE)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                perms.add(Manifest.permission.NEARBY_WIFI_DEVICES)
                perms.add(Manifest.permission.POST_NOTIFICATIONS)
            }
            permissionLauncher.launch(perms.toTypedArray())
        }

        Scaffold(
            topBar = {
                TopAppBar(
                    title = {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Icon(Icons.Default.CloudSync, contentDescription = null, tint = ElectricIndigo)
                            Spacer(modifier = Modifier.width(8.dp))
                            Text("DropLink", fontWeight = FontWeight.Bold, fontSize = 20.sp)
                            Spacer(modifier = Modifier.width(6.dp))
                            Badge(containerColor = ElectricIndigo.copy(alpha = 0.2f)) {
                                Text("PRO", color = ElectricIndigo, fontSize = 10.sp, fontWeight = FontWeight.Bold)
                            }
                        }
                    },
                    actions = {
                        Text(
                            text = "${localDevice.address ?: "Wi-Fi"} : ${localDevice.port}",
                            fontSize = 12.sp,
                            color = AccentIndigo,
                            modifier = Modifier.padding(end = 16.dp)
                        )
                    },
                    colors = TopAppBarDefaults.topAppBarColors(containerColor = ObsidianBackground, titleContentColor = TextPrimary)
                )
            },
            bottomBar = {
                NavigationBar(containerColor = CardSurface) {
                    NavigationBarItem(
                        selected = selectedTab == 0,
                        onClick = { selectedTab = 0 },
                        icon = { Icon(Icons.Default.WifiTethering, contentDescription = "Nearby") },
                        label = { Text("Nearby") },
                        colors = NavigationBarItemDefaults.colors(
                            selectedIconColor = ElectricIndigo,
                            selectedTextColor = ElectricIndigo,
                            indicatorColor = CardHover
                        )
                    )
                    NavigationBarItem(
                        selected = selectedTab == 1,
                        onClick = { selectedTab = 1 },
                        icon = { Icon(Icons.Default.History, contentDescription = "History") },
                        label = { Text("History") },
                        colors = NavigationBarItemDefaults.colors(
                            selectedIconColor = ElectricIndigo,
                            selectedTextColor = ElectricIndigo,
                            indicatorColor = CardHover
                        )
                    )
                    NavigationBarItem(
                        selected = selectedTab == 2,
                        onClick = { selectedTab = 2 },
                        icon = { Icon(Icons.Default.Settings, contentDescription = "Settings") },
                        label = { Text("Settings") },
                        colors = NavigationBarItemDefaults.colors(
                            selectedIconColor = ElectricIndigo,
                            selectedTextColor = ElectricIndigo,
                            indicatorColor = CardHover
                        )
                    )
                }
            },
            containerColor = ObsidianBackground
        ) { innerPadding ->
            Box(modifier = Modifier.padding(innerPadding).fillMaxSize()) {
                when (selectedTab) {
                    0 -> NearbyScreen(
                        peers = peers,
                        stagedCount = stagedUris.size,
                        onSelectFiles = { filePickerLauncher.launch(arrayOf("*/*")) },
                        onOpenDirectIp = { showDirectIpDialog = true },
                        onClearStaged = { stagedUris.clear() },
                        onPeerSelected = { peer ->
                            if (stagedUris.isNotEmpty()) {
                                scope.launch {
                                    val success = transferEngine.sendUris(peer.address ?: "127.0.0.1", peer.port, stagedUris)
                                    if (success) stagedUris.clear()
                                }
                            } else {
                                filePickerLauncher.launch(arrayOf("*/*"))
                            }
                        }
                    )
                    1 -> HistoryScreen()
                    2 -> SettingsScreen(localDevice)
                }

                // Active Sending or Receiving Progress Dialog
                val currentProgress = activeTransfer ?: receivingProgress
                if (currentProgress != null) {
                    TransferProgressDialog(
                        progress = currentProgress,
                        onCancel = {
                            transferEngine.cancelTransfer()
                            receivingProgress = null
                        },
                        onTogglePause = { transferEngine.togglePause() }
                    )
                }

                // Incoming Transfer Prompt Sheet / Alert
                incomingPrompt?.let { manifest ->
                    AlertDialog(
                        onDismissRequest = {
                            incomingPromptCallback?.invoke(false)
                            incomingPrompt = null
                        },
                        title = {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Icon(Icons.Default.FileDownload, contentDescription = null, tint = ElectricIndigo)
                                Spacer(modifier = Modifier.width(8.dp))
                                Text("Incoming Transfer", fontWeight = FontWeight.Bold)
                            }
                        },
                        text = {
                            Column {
                                Text("${manifest.sender.name} (${manifest.sender.platform}) wants to send you:")
                                Spacer(modifier = Modifier.height(8.dp))
                                Card(colors = CardDefaults.cardColors(containerColor = CardSurface), modifier = Modifier.fillMaxWidth()) {
                                    Column(modifier = Modifier.padding(12.dp)) {
                                        Text("${manifest.total_files} files • ${"%.1f".format(manifest.total_size / (1024.0 * 1024.0))} MB", fontWeight = FontWeight.Bold, color = ElectricIndigo)
                                        Text("Saved to Downloads/DropLink", fontSize = 12.sp, color = TextMuted)
                                    }
                                }
                            }
                        },
                        confirmButton = {
                            Button(
                                onClick = {
                                    incomingPromptCallback?.invoke(true)
                                    incomingPrompt = null
                                },
                                colors = ButtonDefaults.buttonColors(containerColor = ElectricIndigo)
                            ) {
                                Text("Accept Transfer")
                            }
                        },
                        dismissButton = {
                            TextButton(
                                onClick = {
                                    incomingPromptCallback?.invoke(false)
                                    incomingPrompt = null
                                }
                            ) {
                                Text("Decline", color = Color(0xFFEF4444))
                            }
                        },
                        containerColor = CardSurface,
                        titleContentColor = TextPrimary,
                        textContentColor = TextPrimary
                    )
                }

                // Direct IP Dialog
                if (showDirectIpDialog) {
                    var ipInput by remember { mutableStateOf("") }
                    var isConnecting by remember { mutableStateOf(false) }

                    AlertDialog(
                        onDismissRequest = { showDirectIpDialog = false },
                        title = { Text("Connect Device via IP", fontWeight = FontWeight.Bold) },
                        text = {
                            Column {
                                Text("Enter the Wi-Fi IP address shown in DropLink on your PC or iPhone:", fontSize = 13.sp, color = TextMuted)
                                Spacer(modifier = Modifier.height(12.dp))
                                OutlinedTextField(
                                    value = ipInput,
                                    onValueChange = { ipInput = it },
                                    label = { Text("e.g. 192.168.1.5") },
                                    singleLine = true,
                                    modifier = Modifier.fillMaxWidth()
                                )
                            }
                        },
                        confirmButton = {
                            Button(
                                onClick = {
                                    if (ipInput.isNotBlank()) {
                                        isConnecting = true
                                        scope.launch {
                                            val peer = discovery.directConnect(ipInput)
                                            isConnecting = false
                                            showDirectIpDialog = false
                                            if (peer != null) {
                                                Toast.makeText(this@MainActivity, "Connected to ${peer.name}!", Toast.LENGTH_SHORT).show()
                                            } else {
                                                Toast.makeText(this@MainActivity, "Could not reach device on $ipInput:52520", Toast.LENGTH_LONG).show()
                                            }
                                        }
                                    }
                                },
                                colors = ButtonDefaults.buttonColors(containerColor = ElectricIndigo),
                                enabled = !isConnecting
                            ) {
                                Text(if (isConnecting) "Connecting..." else "Connect")
                            }
                        },
                        dismissButton = {
                            TextButton(onClick = { showDirectIpDialog = false }) {
                                Text("Cancel", color = TextMuted)
                            }
                        },
                        containerColor = CardSurface,
                        titleContentColor = TextPrimary,
                        textContentColor = TextPrimary
                    )
                }
            }
        }
    }

    @Composable
    fun NearbyScreen(
        peers: List<DeviceInfo>,
        stagedCount: Int,
        onSelectFiles: () -> Unit,
        onOpenDirectIp: () -> Unit,
        onClearStaged: () -> Unit,
        onPeerSelected: (DeviceInfo) -> Unit
    ) {
        LazyColumn(
            modifier = Modifier.fillMaxSize().padding(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp)
        ) {
            item {
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    text = "Send anything. Anywhere nearby.",
                    fontSize = 22.sp,
                    fontWeight = FontWeight.Bold,
                    color = TextPrimary
                )
                Text(
                    text = "Fast, direct local Wi-Fi transfer between Android, iPhone, and Windows.",
                    fontSize = 13.5.sp,
                    color = TextMuted
                )
            }

            item {
                Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    Button(
                        onClick = onSelectFiles,
                        modifier = Modifier.weight(1f),
                        colors = ButtonDefaults.buttonColors(containerColor = ElectricIndigo)
                    ) {
                        Icon(Icons.Default.Add, contentDescription = null)
                        Spacer(modifier = Modifier.width(6.dp))
                        Text("Select Files")
                    }
                    OutlinedButton(
                        onClick = onOpenDirectIp,
                        modifier = Modifier.weight(1f),
                        border = androidx.compose.foundation.BorderStroke(1.dp, ElectricIndigo)
                    ) {
                        Icon(Icons.Default.Wifi, contentDescription = null, tint = AccentIndigo)
                        Spacer(modifier = Modifier.width(6.dp))
                        Text("Connect IP", color = TextPrimary)
                    }
                }
            }

            if (stagedCount > 0) {
                item {
                    Card(
                        colors = CardDefaults.cardColors(containerColor = CardSurface),
                        modifier = Modifier.fillMaxWidth().border(1.dp, ElectricIndigo, RoundedCornerShape(12.dp))
                    ) {
                        Row(
                            modifier = Modifier.padding(14.dp).fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween,
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Column {
                                Text("Ready to send: $stagedCount files", fontWeight = FontWeight.SemiBold, color = TextPrimary)
                                Text("Tap any device below to transfer instantly", fontSize = 12.sp, color = AccentIndigo)
                            }
                            TextButton(onClick = onClearStaged) {
                                Text("Clear", color = TextMuted)
                            }
                        }
                    }
                }
            }

            item {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text("Available Devices (${peers.size})", fontWeight = FontWeight.Bold, fontSize = 16.sp)
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Box(
                            modifier = Modifier.size(8.dp).clip(CircleShape).background(AccentIndigo)
                        )
                        Spacer(modifier = Modifier.width(6.dp))
                        Text("Scanning Wi-Fi", fontSize = 12.sp, color = TextMuted)
                    }
                }
            }

            if (peers.isEmpty()) {
                item {
                    Column(
                        modifier = Modifier.fillMaxWidth().padding(32.dp),
                        horizontalAlignment = Alignment.CenterHorizontally
                    ) {
                        RadarPulseView()
                        Spacer(modifier = Modifier.height(16.dp))
                        Text("Searching for nearby devices...", color = TextMuted, fontSize = 14.sp)
                        Text("Make sure both devices are on the same Wi-Fi", color = TextMuted.copy(alpha = 0.6f), fontSize = 12.sp)
                    }
                }
            } else {
                items(peers) { peer ->
                    PeerCard(peer = peer, hasStaged = stagedCount > 0, onClick = { onPeerSelected(peer) })
                }
            }
        }
    }

    @Composable
    fun RadarPulseView() {
        val infiniteTransition = rememberInfiniteTransition(label = "radar")
        val scale by infiniteTransition.animateFloat(
            initialValue = 0.8f,
            targetValue = 1.3f,
            animationSpec = infiniteRepeatable(
                animation = tween(1500, easing = LinearEasing),
                repeatMode = RepeatMode.Reverse
            ),
            label = "scale"
        )

        Box(
            modifier = Modifier.size(90.dp).border(2.dp, AccentIndigo.copy(alpha = 0.4f), CircleShape),
            contentAlignment = Alignment.Center
        ) {
            Box(
                modifier = Modifier.size((46 * scale).dp).border(2.dp, ElectricIndigo, CircleShape)
            )
            Icon(Icons.Default.Wifi, contentDescription = null, tint = AccentIndigo, modifier = Modifier.size(32.dp))
        }
    }

    @Composable
    fun PeerCard(peer: DeviceInfo, hasStaged: Boolean, onClick: () -> Unit) {
        Card(
            colors = CardDefaults.cardColors(containerColor = CardSurface),
            modifier = Modifier.fillMaxWidth().clickable { onClick() },
            shape = RoundedCornerShape(14.dp)
        ) {
            Row(
                modifier = Modifier.padding(16.dp).fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween
            ) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    val icon = when (peer.platform) {
                        Platform.WINDOWS -> Icons.Default.Laptop
                        Platform.IOS, Platform.ANDROID -> Icons.Default.Smartphone
                        else -> Icons.Default.Computer
                    }
                    Box(
                        modifier = Modifier.size(46.dp).clip(CircleShape).background(CardHover),
                        contentAlignment = Alignment.Center
                    ) {
                        Icon(icon, contentDescription = null, tint = AccentIndigo)
                    }
                    Spacer(modifier = Modifier.width(14.dp))
                    Column {
                        Text(peer.name, fontWeight = FontWeight.SemiBold, fontSize = 15.sp, color = TextPrimary)
                        Text("${peer.platform} • ${peer.address ?: "Local Network"}:${peer.port}", fontSize = 12.sp, color = TextMuted)
                    }
                }

                Button(
                    onClick = onClick,
                    colors = ButtonDefaults.buttonColors(containerColor = ElectricIndigo)
                ) {
                    Text(if (hasStaged) "Send Now" else "Send Files")
                }
            }
        }
    }

    @Composable
    fun TransferProgressDialog(
        progress: LiveTransferProgress,
        onCancel: () -> Unit,
        onTogglePause: () -> Unit
    ) {
        Dialog(onDismissRequest = {}) {
            Card(
                colors = CardDefaults.cardColors(containerColor = CardSurface),
                shape = RoundedCornerShape(16.dp),
                modifier = Modifier.fillMaxWidth().padding(8.dp)
            ) {
                Column(modifier = Modifier.padding(20.dp)) {
                    Text("Transferring Files", fontWeight = FontWeight.Bold, fontSize = 18.sp, color = TextPrimary)
                    Spacer(modifier = Modifier.height(12.dp))
                    Text(progress.currentFileName, fontSize = 14.sp, color = TextPrimary)
                    Text("File ${progress.currentFileIndex + 1} of ${progress.totalFiles}", fontSize = 12.sp, color = TextMuted)

                    val pct = if (progress.totalBytes > 0) progress.transferredBytes.toFloat() / progress.totalBytes.toFloat() else 0f
                    Spacer(modifier = Modifier.height(12.dp))
                    LinearProgressIndicator(
                        progress = { pct },
                        modifier = Modifier.fillMaxWidth().height(8.dp).clip(RoundedCornerShape(4.dp)),
                        color = ElectricIndigo,
                        trackColor = CardHover
                    )

                    Spacer(modifier = Modifier.height(12.dp))
                    val transferredMB = "%.1f".format(progress.transferredBytes / (1024.0 * 1024.0))
                    val totalMB = "%.1f".format(progress.totalBytes / (1024.0 * 1024.0))
                    val speedStr = "%.1f MB/s".format(progress.speedBytesPerSec / (1024 * 1024))
                    val etaStr = progress.estimatedSecondsRemaining?.let { "~${it}s remaining" } ?: "Calculating..."
                    Text("$transferredMB MB / $totalMB MB • $speedStr • $etaStr", fontSize = 12.sp, color = TextMuted)

                    Spacer(modifier = Modifier.height(16.dp))
                    Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                        TextButton(onClick = onTogglePause) {
                            Text("Pause", color = AccentIndigo)
                        }
                        TextButton(onClick = onCancel) {
                            Text("Cancel", color = Color(0xFFEF4444))
                        }
                    }
                }
            }
        }
    }

    @Composable
    fun HistoryScreen() {
        Column(modifier = Modifier.fillMaxSize().padding(16.dp)) {
            Text("Transfer History", fontSize = 22.sp, fontWeight = FontWeight.Bold, color = TextPrimary)
            Spacer(modifier = Modifier.height(12.dp))
            Card(colors = CardDefaults.cardColors(containerColor = CardSurface), modifier = Modifier.fillMaxWidth()) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text("Downloads Directory", fontWeight = FontWeight.SemiBold, color = TextPrimary)
                    Text("Received files are stored in your device's Downloads/DropLink folder.", color = TextMuted, fontSize = 13.sp)
                }
            }
        }
    }

    @Composable
    fun SettingsScreen(device: DeviceInfo) {
        val clipboardManager = LocalClipboardManager.current
        var autoAccept by remember { mutableStateOf(false) }
        var showNameDialog by remember { mutableStateOf(false) }
        var currentName by remember { mutableStateOf(device.name) }

        Column(
            modifier = Modifier.fillMaxSize().padding(16.dp).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(16.dp)
        ) {
            Text("Settings", fontSize = 22.sp, fontWeight = FontWeight.Bold, color = TextPrimary)

            // 1. Device Profile Card
            Card(colors = CardDefaults.cardColors(containerColor = CardSurface), modifier = Modifier.fillMaxWidth()) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Column(modifier = Modifier.weight(1f)) {
                            Text("Device Name", fontWeight = FontWeight.SemiBold, color = TextPrimary)
                            Text(currentName, color = TextMuted, fontSize = 14.sp)
                        }
                        IconButton(onClick = { showNameDialog = true }) {
                            Icon(Icons.Default.Edit, contentDescription = "Edit Name", tint = AccentIndigo)
                        }
                    }

                    HorizontalDivider(modifier = Modifier.padding(vertical = 12.dp), color = CardHover)

                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Column(modifier = Modifier.weight(1f)) {
                            Text("Wi-Fi IP Address", fontWeight = FontWeight.SemiBold, color = TextPrimary)
                            Text("${device.address ?: "127.0.0.1"} : ${device.port}", color = ElectricIndigo, fontSize = 14.sp)
                        }
                        Button(
                            onClick = {
                                clipboardManager.setText(AnnotatedString(device.address ?: "127.0.0.1"))
                                Toast.makeText(this@MainActivity, "Copied IP to clipboard", Toast.LENGTH_SHORT).show()
                            },
                            colors = ButtonDefaults.buttonColors(containerColor = CardHover),
                            contentPadding = PaddingValues(horizontal = 12.dp, vertical = 6.dp)
                        ) {
                            Text("Copy IP", fontSize = 12.sp, color = TextPrimary)
                        }
                    }
                }
            }

            // 2. Transfer Preferences Card
            Card(colors = CardDefaults.cardColors(containerColor = CardSurface), modifier = Modifier.fillMaxWidth()) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text("Transfer Preferences", fontWeight = FontWeight.Bold, fontSize = 16.sp, color = TextPrimary)
                    Spacer(modifier = Modifier.height(12.dp))

                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Column(modifier = Modifier.weight(1f)) {
                            Text("Save Location", fontWeight = FontWeight.SemiBold, color = TextPrimary)
                            Text("Downloads/DropLink", color = TextMuted, fontSize = 13.sp)
                        }
                        Text("Default", fontSize = 12.sp, color = AccentIndigo)
                    }

                    HorizontalDivider(modifier = Modifier.padding(vertical = 12.dp), color = CardHover)

                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Column(modifier = Modifier.weight(1f)) {
                            Text("Auto-Accept Transfers", fontWeight = FontWeight.SemiBold, color = TextPrimary)
                            Text("Automatically accept files from trusted devices", color = TextMuted, fontSize = 12.sp)
                        }
                        Switch(
                            checked = autoAccept,
                            onCheckedChange = { autoAccept = it },
                            colors = SwitchDefaults.colors(checkedThumbColor = ElectricIndigo)
                        )
                    }
                }
            }

            // 3. Network & System Permissions Card
            Card(colors = CardDefaults.cardColors(containerColor = CardSurface), modifier = Modifier.fillMaxWidth()) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text("Network & Status", fontWeight = FontWeight.Bold, fontSize = 16.sp, color = TextPrimary)
                    Spacer(modifier = Modifier.height(10.dp))
                    PermissionStatusRow(title = "Local Network (Wi-Fi)", status = "Connected")
                    PermissionStatusRow(title = "HTTP Receiver Server", status = "Port 52520 Active")
                    PermissionStatusRow(title = "Foreground Sync Service", status = "Ready")
                }
            }

            // 4. About Card
            Card(colors = CardDefaults.cardColors(containerColor = CardSurface), modifier = Modifier.fillMaxWidth()) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text("About DropLink", fontWeight = FontWeight.Bold, fontSize = 16.sp, color = TextPrimary)
                    Spacer(modifier = Modifier.height(6.dp))
                    Text("DropLink Android v1.0.0 Pro", fontWeight = FontWeight.SemiBold, color = ElectricIndigo)
                    Text("Cross-Platform Local Network Peer-to-Peer Transfer", fontSize = 12.sp, color = TextMuted)
                    Spacer(modifier = Modifier.height(4.dp))
                    Text("100% Private. No Cloud. Zero Compression.", fontSize = 12.sp, color = TextMuted)
                }
            }

            Spacer(modifier = Modifier.height(24.dp))
        }

        if (showNameDialog) {
            var tempName by remember { mutableStateOf(currentName) }
            AlertDialog(
                onDismissRequest = { showNameDialog = false },
                title = { Text("Rename Device") },
                text = {
                    OutlinedTextField(
                        value = tempName,
                        onValueChange = { tempName = it },
                        label = { Text("Device Name") },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth()
                    )
                },
                confirmButton = {
                    Button(
                        onClick = {
                            if (tempName.isNotBlank()) {
                                currentName = tempName
                                showNameDialog = false
                            }
                        },
                        colors = ButtonDefaults.buttonColors(containerColor = ElectricIndigo)
                    ) {
                        Text("Save")
                    }
                },
                dismissButton = {
                    TextButton(onClick = { showNameDialog = false }) {
                        Text("Cancel")
                    }
                }
            )
        }
    }

    @Composable
    private fun PermissionStatusRow(title: String, status: String) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text(title, fontSize = 13.sp, color = TextPrimary)
            Text(status, fontSize = 12.sp, color = AccentIndigo, fontWeight = FontWeight.SemiBold)
        }
    }
}
