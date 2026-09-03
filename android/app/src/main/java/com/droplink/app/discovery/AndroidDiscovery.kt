package com.droplink.app.discovery

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.net.wifi.WifiManager
import com.droplink.app.protocol.DeviceInfo
import com.droplink.app.protocol.DiscoveryBeacon
import com.google.gson.Gson
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import okhttp3.OkHttpClient
import okhttp3.Request
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.Inet4Address
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.NetworkInterface
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit

class AndroidDiscovery(
    private val context: Context,
    private val localDevice: DeviceInfo
) {
    private val gson = Gson()
    private val discoveredMap = ConcurrentHashMap<String, Pair<DeviceInfo, Long>>()

    private val _peersFlow = MutableStateFlow<List<DeviceInfo>>(emptyList())
    val peersFlow: StateFlow<List<DeviceInfo>> = _peersFlow.asStateFlow()

    private var isRunning = false
    private var discoveryScope: CoroutineScope? = null
    private var nsdManager: NsdManager? = null
    private var multicastLock: WifiManager.MulticastLock? = null

    private val httpClient = OkHttpClient.Builder()
        .connectTimeout(3, TimeUnit.SECONDS)
        .readTimeout(3, TimeUnit.SECONDS)
        .build()

    fun start() {
        if (isRunning) return
        isRunning = true
        discoveryScope = CoroutineScope(Dispatchers.IO + SupervisorJob())

        // Acquire Android Wi-Fi MulticastLock so the OS doesn't drop incoming UDP broadcasts
        try {
            val wifi = context.applicationContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager
            multicastLock = wifi?.createMulticastLock("droplink_multicast_lock")?.apply {
                setReferenceCounted(true)
                acquire()
            }
        } catch (e: Exception) {
            e.printStackTrace()
        }

        // 1. Start UDP Beacon Listener
        discoveryScope?.launch {
            runUdpListener()
        }

        // 2. Start UDP Beacon Announcer
        discoveryScope?.launch {
            runUdpAnnouncer()
        }

        // 3. Start Stale Peer Reaper
        discoveryScope?.launch {
            runReaper()
        }

        // 4. Register NsdManager (mDNS / DNS-SD)
        try {
            nsdManager = context.getSystemService(Context.NSD_SERVICE) as? NsdManager
            registerNsdService()
            discoverNsdServices()
        } catch (e: Exception) {
            e.printStackTrace()
        }
    }

    fun stop() {
        isRunning = false
        discoveryScope?.cancel()
        discoveryScope = null
        try {
            if (multicastLock?.isHeld == true) {
                multicastLock?.release()
            }
        } catch (_: Exception) {}
        multicastLock = null
    }

    suspend fun directConnect(ip: String): DeviceInfo? = withContext(Dispatchers.IO) {
        try {
            val cleanIp = ip.trim().removePrefix("http://").substringBefore(":")
            val req = Request.Builder().url("http://$cleanIp:52520/api/v1/ping").get().build()
            val resp = httpClient.newCall(req).execute()
            if (resp.isSuccessful) {
                val body = resp.body?.string() ?: return@withContext null
                val peer = gson.fromJson(body, DeviceInfo::class.java)
                peer.address = cleanIp
                discoveredMap.entries.removeIf { entry ->
                    val existing = entry.value.first
                    existing.address == cleanIp || (existing.name == peer.name && existing.platform == peer.platform)
                }
                discoveredMap[peer.id] = Pair(peer, System.currentTimeMillis())
                updateFlow()
                return@withContext peer
            }
        } catch (_: Exception) {}
        null
    }

    private suspend fun runUdpListener() = withContext(Dispatchers.IO) {
        var socket: DatagramSocket? = null
        try {
            socket = DatagramSocket(null).apply {
                reuseAddress = true
                broadcast = true
                bind(InetSocketAddress(52520))
            }
            val buffer = ByteArray(4096)

            while (isRunning) {
                val packet = DatagramPacket(buffer, buffer.size)
                socket.receive(packet)

                val json = String(packet.data, 0, packet.length, Charsets.UTF_8)
                try {
                    val beacon = gson.fromJson(json, DiscoveryBeacon::class.java)
                    if (beacon != null && beacon.magic == "DROPLINK_BEACON" && beacon.device.id != localDevice.id) {
                        val peer = beacon.device
                        val senderIp = packet.address.hostAddress
                        peer.address = senderIp

                        // Deduplicate: Remove any existing entry with the same IP or same name+platform
                        discoveredMap.entries.removeIf { entry ->
                            val existing = entry.value.first
                            (existing.address != null && existing.address == senderIp) ||
                            (existing.name == peer.name && existing.platform == peer.platform)
                        }

                        discoveredMap[peer.id] = Pair(peer, System.currentTimeMillis())
                        updateFlow()
                    }
                } catch (_: Exception) {}
            }
        } catch (e: Exception) {
            if (isRunning) e.printStackTrace()
        } finally {
            socket?.close()
        }
    }

    private suspend fun runUdpAnnouncer() = withContext(Dispatchers.IO) {
        var socket: DatagramSocket? = null
        try {
            socket = DatagramSocket().apply { broadcast = true }
            val globalBroadcast = InetAddress.getByName("255.255.255.255")
            val subnetBroadcast = getSubnetBroadcastAddress()

            while (isRunning) {
                val beacon = DiscoveryBeacon(
                    device = localDevice,
                    timestamp = System.currentTimeMillis() / 1000
                )
                val json = gson.toJson(beacon)
                val bytes = json.toByteArray(Charsets.UTF_8)

                // 1. Send to global broadcast 255.255.255.255
                try {
                    socket.send(DatagramPacket(bytes, bytes.size, globalBroadcast, 52520))
                } catch (_: Exception) {}

                // 2. Send to directed subnet broadcast (e.g. 192.168.1.255)
                if (subnetBroadcast != null) {
                    try {
                        socket.send(DatagramPacket(bytes, bytes.size, subnetBroadcast, 52520))
                    } catch (_: Exception) {}
                }

                delay(2000)
            }
        } catch (e: Exception) {
            if (isRunning) e.printStackTrace()
        } finally {
            socket?.close()
        }
    }

    private fun getSubnetBroadcastAddress(): InetAddress? {
        try {
            val interfaces = NetworkInterface.getNetworkInterfaces()
            for (intf in interfaces) {
                if (intf.isLoopback || !intf.isUp) continue
                for (linkAddr in intf.interfaceAddresses) {
                    val broadcast = linkAddr.broadcast
                    if (broadcast != null && broadcast is Inet4Address) {
                        return broadcast
                    }
                }
            }
        } catch (_: Exception) {}
        return null
    }

    private suspend fun runReaper() = withContext(Dispatchers.IO) {
        val timeout = 12_000L
        while (isRunning) {
            delay(4000)
            val now = System.currentTimeMillis()
            var changed = false
            for ((id, pair) in discoveredMap) {
                if (now - pair.second > timeout) {
                    discoveredMap.remove(id)
                    changed = true
                }
            }
            if (changed) updateFlow()
        }
    }

    private fun updateFlow() {
        _peersFlow.value = discoveredMap.values.map { it.first }.sortedBy { it.name }
    }

    private fun registerNsdService() {
        val serviceInfo = NsdServiceInfo().apply {
            serviceName = "DropLink-${localDevice.name}"
            serviceType = "_droplink._tcp."
            port = localDevice.port
        }
        try {
            nsdManager?.registerService(serviceInfo, NsdManager.PROTOCOL_DNS_SD, object : NsdManager.RegistrationListener {
                override fun onServiceRegistered(p0: NsdServiceInfo?) {}
                override fun onRegistrationFailed(p0: NsdServiceInfo?, p1: Int) {}
                override fun onServiceUnregistered(p0: NsdServiceInfo?) {}
                override fun onUnregistrationFailed(p0: NsdServiceInfo?, p1: Int) {}
            })
        } catch (_: Exception) {}
    }

    private fun discoverNsdServices() {
        try {
            nsdManager?.discoverServices("_droplink._tcp.", NsdManager.PROTOCOL_DNS_SD, object : NsdManager.DiscoveryListener {
                override fun onStartDiscoveryFailed(p0: String?, p1: Int) {}
                override fun onStopDiscoveryFailed(p0: String?, p1: Int) {}
                override fun onDiscoveryStarted(p0: String?) {}
                override fun onDiscoveryStopped(p0: String?) {}
                override fun onServiceFound(serviceInfo: NsdServiceInfo) {
                    nsdManager?.resolveService(serviceInfo, object : NsdManager.ResolveListener {
                        override fun onResolveFailed(p0: NsdServiceInfo?, p1: Int) {}
                        override fun onServiceResolved(resolved: NsdServiceInfo) {
                            val host = resolved.host?.hostAddress ?: return
                            val port = resolved.port
                            val name = resolved.serviceName.removePrefix("DropLink-")
                            val id = "mdns-$host-$port"
                            val peer = DeviceInfo(
                                id = id,
                                name = name,
                                platform = com.droplink.app.protocol.Platform.UNKNOWN,
                                version = "1.0.0",
                                port = port,
                                fingerprint = "",
                                address = host
                            )
                            discoveredMap[id] = Pair(peer, System.currentTimeMillis())
                            updateFlow()
                        }
                    })
                }
                override fun onServiceLost(serviceInfo: NsdServiceInfo) {}
            })
        } catch (_: Exception) {}
    }
}
