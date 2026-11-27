package com.xent.app

import android.content.ContentResolver
import android.content.Context
import android.net.Uri
import android.net.wifi.WifiManager
import android.provider.OpenableColumns
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.json.JSONObject
import java.io.File
import java.io.FileInputStream
import java.io.InputStream
import java.io.RandomAccessFile
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.Inet4Address
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.MulticastSocket
import java.net.NetworkInterface
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Semaphore
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Coordinates Xent LAN discovery (UDP multicast/broadcast + HTTP handshake), encrypted peer messaging,
 * offline retry queue, and chunked file transfers (Pause/Resume/Cancel + Favorites auto-accept).
 */
class XentEngine(
    dataDir: File,
    val downloadDir: File,
    defaultAlias: String,
    private val context: Context? = null,
    private val preferredPort: Int = 53317,
    private val enableUdpDiscovery: Boolean = true
) {
    companion object {
        const val MULTICAST_ADDR = "224.0.0.167"
        const val DISCOVERY_PORT = 53317
        const val CHUNK_SIZE = 512 * 1024 // 512 KB, matches Rust UPLOAD_CHUNK_SIZE
        const val MAX_PARALLEL_UPLOADS = 2
    }

    val store = XentStore(dataDir, defaultAlias)
    val tls: XentTlsBundle = XentCrypto.loadOrCreateBundle(dataDir)
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    private data class UploadControl(
        val cancelled: AtomicBoolean = AtomicBoolean(false),
        val paused: AtomicBoolean = AtomicBoolean(false),
        val started: AtomicBoolean = AtomicBoolean(false),
        val source: QueuedUploadSource,
        val openFile: RandomAccessFile? = null
    )

    private val uploadControls = ConcurrentHashMap<String, UploadControl>()
    private val receiveFiles = ConcurrentHashMap<String, RandomAccessFile>()
    private val receiveLock = Any()
    private val uploadPermits = Semaphore(MAX_PARALLEL_UPLOADS)
    private val started = AtomicBoolean(false)

    private var server: XentHttpServer? = null
    private var multicastLock: WifiManager.MulticastLock? = null
    private var udpSocket: MulticastSocket? = null

    var onIncomingNotify: ((title: String, body: String) -> Unit)? = null

    val boundPort: Int
        get() = server?.boundPort ?: store.getSettings().port

    fun start(): Int {
        if (!started.compareAndSet(false, true)) return boundPort
        downloadDir.mkdirs()

        val httpServer = XentHttpServer(tls.serverContext) { method, path, query, remoteIp, body ->
            handleHttpRequest(method, path, query, remoteIp, body)
        }
        val port = httpServer.start(store.getSettings().port.takeIf { it > 0 } ?: preferredPort)
        server = httpServer
        store.updateSettings { it.copy(port = port) }

        if (enableUdpDiscovery) {
            acquireMulticastLock()
            startUdpListener()
            startDiscoveryLoop()
        }
        startQueueFlusherLoop()
        return port
    }

    fun stop() {
        started.set(false)
        if (enableUdpDiscovery) {
            runCatching { broadcastPresence("bye") }
        }
        runCatching { udpSocket?.close() }
        udpSocket = null
        releaseMulticastLock()
        runCatching { server?.stop() }
        server = null
        synchronized(receiveLock) {
            receiveFiles.values.forEach { runCatching { it.close() } }
            receiveFiles.clear()
        }
        scope.cancel()
    }

    // ---- HTTP Server Router ----

    private fun handleHttpRequest(
        method: String,
        path: String,
        query: Map<String, String>,
        remoteIp: String,
        body: ByteArray
    ): XentHttpServer.HttpResponse {
        if (method == "GET" && path == "/api/ping") {
            return XentHttpServer.HttpResponse.text(200, "OK", "pong")
        }
        if (method == "GET" && path == "/api/info") {
            return XentHttpServer.HttpResponse.okJson(buildDeviceInfoJson())
        }
        if (method == "POST" && path == "/api/discovery/hello") {
            val json = JSONObject(String(body, Charsets.UTF_8))
            val deviceId = json.optString("device_id")
            if (deviceId.isNotBlank() && deviceId != store.getSettings().deviceId && !store.isBlocked(deviceId)) {
                val peer = PeerInfo(
                    deviceId = deviceId,
                    alias = json.optString("alias", "Device"),
                    deviceType = json.optString("device_type", "desktop"),
                    os = json.optString("os", "").ifBlank { null },
                    ip = remoteIp,
                    port = json.optInt("port", 53317),
                    fingerprint = json.optString("fingerprint", ""),
                    status = json.optString("status", "available"),
                    statusMessage = json.optString("status_message", ""),
                    lastSeenMs = System.currentTimeMillis(),
                    online = true
                )
                store.upsertPeer(peer)
            }
            return XentHttpServer.HttpResponse.okJson(buildDeviceInfoJson())
        }
        if (method == "POST" && path == "/api/message") {
            val json = JSONObject(String(body, Charsets.UTF_8))
            val senderId = json.optString("sender_id")
            if (store.isBlocked(senderId)) {
                return XentHttpServer.HttpResponse.error(403, "Forbidden", "Sender blocked")
            }
            val msgId = json.optString("id").ifBlank { UUID.randomUUID().toString() }
            val created = json.optLong("created_at", System.currentTimeMillis())
            val senderName = json.optString("sender_name", "Device")
            val content = json.optString("content")
            val quote = if (json.isNull("quote")) null else json.optString("quote").ifBlank { null }
            val existingPeer = store.getPeer(senderId) ?: store.findPeerByIp(remoteIp)
            if (senderId.isNotBlank()) {
                store.upsertPeer(
                    (existingPeer ?: PeerInfo(deviceId = senderId, alias = senderName, ip = remoteIp))
                        .copy(alias = senderName, ip = remoteIp, online = true, lastSeenMs = System.currentTimeMillis())
                )
            }
            val msg = ChatMessage(
                id = msgId,
                threadPeerId = senderId,
                senderId = senderId,
                senderName = senderName,
                receiverId = json.optString("receiver_id", store.getSettings().deviceId),
                receiverName = json.optString("receiver_name", store.getSettings().alias),
                content = content,
                status = "Delivered, ${XentFormat.formatTime(created)}",
                createdAt = created,
                isIncoming = true,
                isRead = false,
                quote = quote
            )
            val isNew = store.saveIncomingMessage(msg)
            if (isNew) {
                onIncomingNotify?.invoke(senderName, content)
            }
            return XentHttpServer.HttpResponse.okJson(
                JSONObject()
                    .put("success", true)
                    .put("id", msgId)
                    .put("status", "delivered")
                    .put("timestamp", System.currentTimeMillis())
            )
        }
        if (method == "POST" && path == "/api/message/typing") {
            val json = JSONObject(String(body, Charsets.UTF_8))
            val senderId = json.optString("sender_id")
            if (senderId.isNotBlank()) store.setPeerTyping(senderId)
            return XentHttpServer.HttpResponse.okJson(JSONObject().put("success", true))
        }
        if (method == "POST" && path == "/api/message/read") {
            val json = JSONObject(String(body, Charsets.UTF_8))
            val peerId = json.optString("peer_id")
            if (peerId.isNotBlank()) {
                store.markOutgoingRead(peerId, XentFormat.formatTime(System.currentTimeMillis()))
            }
            return XentHttpServer.HttpResponse.okJson(JSONObject().put("success", true))
        }
        if (method == "POST" && path == "/api/message/edit") {
            val json = JSONObject(String(body, Charsets.UTF_8))
            store.applyEdit(json.optString("message_id"), json.optString("new_content"))
            return XentHttpServer.HttpResponse.okJson(JSONObject().put("success", true))
        }
        if (method == "POST" && path == "/api/message/delete") {
            val json = JSONObject(String(body, Charsets.UTF_8))
            store.applyDelete(json.optString("message_id"))
            return XentHttpServer.HttpResponse.okJson(JSONObject().put("success", true))
        }
        if (method == "POST" && path == "/api/message/reaction") {
            val json = JSONObject(String(body, Charsets.UTF_8))
            store.toggleReaction(
                json.optString("message_id"),
                json.optString("user_name"),
                json.optString("emoji")
            )
            return XentHttpServer.HttpResponse.okJson(JSONObject().put("success", true))
        }
        if (method == "POST" && path == "/api/files/request") {
            val json = JSONObject(String(body, Charsets.UTF_8))
            val transferId = json.optString("transfer_id").ifBlank { UUID.randomUUID().toString() }
            val senderId = json.optString("sender_id")
            val senderName = json.optString("sender_name", "Device")
            val fileName = json.optString("file_name", "file")
            val fileSize = json.optLong("file_size", 0L)
            val senderPort = json.optInt("sender_port", 53317)

            if (store.isBlocked(senderId)) {
                return XentHttpServer.HttpResponse.okJson(
                    JSONObject()
                        .put("success", true)
                        .put("transfer_id", transferId)
                        .put("auto_accepted", false)
                        .put("auto_declined", true)
                )
            }

            val autoAccept = store.getSettings().autoAcceptFavorites && store.isFavorite(senderId)
            val reserved = if (autoAccept) XentFormat.reserveUniqueFile(downloadDir, fileName) else null
            if (reserved != null) {
                synchronized(receiveLock) {
                    receiveFiles[transferId] = RandomAccessFile(reserved, "rw")
                }
            }

            val item = TransferItem(
                transferId = transferId,
                fileName = fileName,
                fileSize = fileSize,
                transferredBytes = 0L,
                status = if (autoAccept) {
                    if (fileSize == 0L) TransferStatus.COMPLETED else TransferStatus.TRANSFERRING
                } else {
                    TransferStatus.PENDING
                },
                isIncoming = true,
                senderId = senderId,
                senderName = senderName,
                receiverId = store.getSettings().deviceId,
                receiverName = store.getSettings().alias,
                peerIp = remoteIp,
                peerPort = senderPort,
                filePath = reserved?.absolutePath
            )
            store.putTransfer(item)
            if (autoAccept && fileSize == 0L) {
                closeReceiveFile(transferId)
            }
            if (!autoAccept) {
                onIncomingNotify?.invoke("Incoming file from $senderName", "$fileName (${XentFormat.formatBytes(fileSize)})")
            }
            return XentHttpServer.HttpResponse.okJson(
                JSONObject()
                    .put("success", true)
                    .put("transfer_id", transferId)
                    .put("auto_accepted", autoAccept)
                    .put("auto_declined", false)
            )
        }
        if (method == "POST" && path == "/api/files/response") {
            val json = JSONObject(String(body, Charsets.UTF_8))
            val transferId = json.optString("transfer_id")
            val accepted = json.optBoolean("accepted", false)
            if (accepted) {
                startUploadAsync(transferId)
            } else {
                uploadControls[transferId]?.cancelled?.set(true)
                store.updateTransfer(transferId) { it.copy(status = TransferStatus.DECLINED) }
            }
            return XentHttpServer.HttpResponse.okJson(JSONObject().put("success", true))
        }
        if (method == "POST" && path.startsWith("/api/files/chunk/")) {
            val transferId = path.removePrefix("/api/files/chunk/")
            val offset = query["offset"]?.toLongOrNull() ?: 0L
            val cur = store.getTransfer(transferId)
                ?: return XentHttpServer.HttpResponse.error(404, "Not Found", "Unknown transfer")
            if (cur.status == TransferStatus.CANCELLED || cur.status == TransferStatus.DECLINED) {
                return XentHttpServer.HttpResponse.error(409, "Conflict", "Transfer closed")
            }
            if (cur.status != TransferStatus.TRANSFERRING && cur.status != TransferStatus.PAUSED) {
                return XentHttpServer.HttpResponse.error(403, "Forbidden", "Transfer not accepted")
            }
            val raf = synchronized(receiveLock) { receiveFiles[transferId] }
                ?: return XentHttpServer.HttpResponse.error(500, "Internal Server Error", "Receive file closed")
            synchronized(raf) {
                raf.seek(offset)
                raf.write(body)
            }
            val newTransferred = (offset + body.size).coerceAtMost(cur.fileSize.coerceAtLeast(offset + body.size))
            val done = cur.fileSize > 0 && newTransferred >= cur.fileSize
            val elapsedSec = ((System.currentTimeMillis() - cur.createdAt).coerceAtLeast(100L)) / 1000.0
            val speed = (newTransferred / 1_048_576.0) / elapsedSec
            store.updateTransfer(transferId) {
                it.copy(
                    transferredBytes = newTransferred,
                    speedMbps = speed,
                    status = if (done) TransferStatus.COMPLETED else TransferStatus.TRANSFERRING
                )
            }
            if (done) closeReceiveFile(transferId)
            return XentHttpServer.HttpResponse.okJson(
                JSONObject().put("success", true).put("transferred", newTransferred)
            )
        }
        if (method == "POST" && path.startsWith("/api/files/pause/")) {
            val transferId = path.removePrefix("/api/files/pause/")
            uploadControls[transferId]?.paused?.set(true)
            store.updateTransfer(transferId) {
                if (it.status == TransferStatus.TRANSFERRING || it.status == TransferStatus.PENDING) {
                    it.copy(status = TransferStatus.PAUSED, speedMbps = 0.0)
                } else it
            }
            return XentHttpServer.HttpResponse.okJson(JSONObject().put("success", true))
        }
        if (method == "POST" && path.startsWith("/api/files/resume/")) {
            val transferId = path.removePrefix("/api/files/resume/")
            uploadControls[transferId]?.paused?.set(false)
            store.updateTransfer(transferId) {
                if (it.status == TransferStatus.PAUSED) {
                    it.copy(status = TransferStatus.TRANSFERRING)
                } else it
            }
            return XentHttpServer.HttpResponse.okJson(JSONObject().put("success", true))
        }
        if (method == "POST" && path.startsWith("/api/files/cancel/")) {
            val transferId = path.removePrefix("/api/files/cancel/")
            uploadControls[transferId]?.cancelled?.set(true)
            closeReceiveFile(transferId)
            store.updateTransfer(transferId) {
                if (!it.status.isTerminal()) it.copy(status = TransferStatus.CANCELLED, speedMbps = 0.0) else it
            }
            return XentHttpServer.HttpResponse.okJson(JSONObject().put("success", true))
        }

        return XentHttpServer.HttpResponse.error(404, "Not Found", "Unknown endpoint $path")
    }

    private fun closeReceiveFile(transferId: String) {
        synchronized(receiveLock) {
            receiveFiles.remove(transferId)?.let { runCatching { it.close() } }
        }
    }

    private fun buildDeviceInfoJson(): JSONObject {
        val s = store.getSettings()
        return JSONObject()
            .put("device_id", s.deviceId)
            .put("alias", s.alias)
            .put("device_type", s.deviceType)
            .put("port", boundPort)
            .put("fingerprint", tls.fingerprintHex)
            .put("protocol_version", 1)
            .put("status", s.status)
            .put("status_message", s.statusMessage)
            .put("os", s.os)
    }

    // ---- Discovery & Peer Handshake ----

    suspend fun addPeerByIp(address: String): Result<PeerInfo> = kotlinx.coroutines.withContext(Dispatchers.IO) {
        runCatching {
            val trimmed = address.trim()
            val (ip, port) = if (trimmed.contains(':')) {
                val p = trimmed.split(':')
                p[0] to (p.getOrNull(1)?.toIntOrNull() ?: 53317)
            } else {
                trimmed to 53317
            }
            val startMs = System.nanoTime()
            val (code, body) = XentCrypto.postJson(
                ip, port, "/api/discovery/hello", buildDeviceInfoJson().toString(), tls.clientContext
            )
            if (code !in 200..299) error("Peer returned HTTP $code")
            val rttMs = ((System.nanoTime() - startMs) / 1_000_000L).coerceAtLeast(1L)
            val json = JSONObject(body)
            val peer = PeerInfo(
                deviceId = json.optString("device_id"),
                alias = json.optString("alias", "Device"),
                deviceType = json.optString("device_type", "desktop"),
                os = json.optString("os", "").ifBlank { null },
                ip = ip,
                port = json.optInt("port", port),
                fingerprint = json.optString("fingerprint", ""),
                status = json.optString("status", "available"),
                statusMessage = json.optString("status_message", ""),
                lastSeenMs = System.currentTimeMillis(),
                latencyMs = rttMs,
                online = true
            )
            store.upsertPeer(peer)
            store.updateSettings { s ->
                val target = "$ip:${peer.port}"
                if (s.manualPeers.contains(target)) s else s.copy(manualPeers = s.manualPeers + target)
            }
            peer
        }
    }

    fun rescanNetwork() {
        scope.launch {
            runCatching { broadcastPresence("announce") }
            val manual = store.getSettings().manualPeers
            manual.forEach { addr -> launch { addPeerByIp(addr) } }
            probeActiveSubnets()
        }
    }

    private fun acquireMulticastLock() {
        val wm = context?.applicationContext?.getSystemService(Context.WIFI_SERVICE) as? WifiManager ?: return
        runCatching {
            multicastLock = wm.createMulticastLock("xent-discovery").apply {
                setReferenceCounted(false)
                acquire()
            }
        }
    }

    private fun releaseMulticastLock() {
        runCatching {
            if (multicastLock?.isHeld == true) multicastLock?.release()
        }
        multicastLock = null
    }

    private fun startUdpListener() {
        scope.launch {
            val sock = runCatching {
                MulticastSocket(null).apply {
                    reuseAddress = true
                    broadcast = true
                    bind(InetSocketAddress(DISCOVERY_PORT))
                    runCatching { joinGroup(InetAddress.getByName(MULTICAST_ADDR)) }
                }
            }.getOrNull() ?: return@launch
            udpSocket = sock
            val buf = ByteArray(4096)
            while (started.get()) {
                val pkt = DatagramPacket(buf, buf.size)
                val received = runCatching {
                    sock.receive(pkt)
                    true
                }.getOrDefault(false)
                if (!received) break

                val senderIp = pkt.address?.hostAddress?.removePrefix("::ffff:") ?: continue
                val text = String(pkt.data, 0, pkt.length, Charsets.UTF_8)
                runCatching {
                    val json = JSONObject(text)
                    val deviceId = json.optString("device_id")
                    val me = store.getSettings()
                    if (deviceId.isBlank() || deviceId == me.deviceId || store.isBlocked(deviceId)) return@runCatching
                    val msgType = json.optString("msg_type", "announce")
                    if (msgType == "bye") {
                        store.markPeerOffline(deviceId)
                        return@runCatching
                    }
                    val peerPort = json.optInt("port", 53317)
                    val peer = PeerInfo(
                        deviceId = deviceId,
                        alias = json.optString("alias", "Device"),
                        deviceType = json.optString("device_type", "desktop"),
                        os = json.optString("os", "").ifBlank { null },
                        ip = senderIp,
                        port = peerPort,
                        fingerprint = json.optString("fingerprint", ""),
                        status = json.optString("status", "available"),
                        statusMessage = json.optString("status_message", ""),
                        lastSeenMs = System.currentTimeMillis(),
                        online = true
                    )
                    store.upsertPeer(peer)
                    if (msgType == "announce") {
                        launch {
                            runCatching {
                                val t0 = System.nanoTime()
                                val (c, _) = XentCrypto.postJson(
                                    senderIp, peerPort, "/api/discovery/hello",
                                    buildDeviceInfoJson().toString(), tls.clientContext, 2000, 3000
                                )
                                if (c in 200..299) {
                                    val rtt = ((System.nanoTime() - t0) / 1_000_000L).coerceAtLeast(1L)
                                    store.upsertPeer(peer.copy(latencyMs = rtt, lastSeenMs = System.currentTimeMillis()))
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    private fun startDiscoveryLoop() {
        scope.launch {
            while (started.get()) {
                if (store.getSettings().discoveryEnabled && store.getSettings().status != "offline") {
                    runCatching { broadcastPresence("announce") }
                    store.getSettings().manualPeers.forEach { addr ->
                        launch { addPeerByIp(addr) }
                    }
                    // Ping online peers to keep RTT fresh
                    store.getPeers().filter { it.online }.forEach { peer ->
                        launch {
                            val t0 = System.nanoTime()
                            val ok = runCatching {
                                val (c, _) = XentCrypto.getText(peer.ip, peer.port, "/api/ping", tls.clientContext, 1500, 2000)
                                c == 200
                            }.getOrDefault(false)
                            if (ok) {
                                val rtt = ((System.nanoTime() - t0) / 1_000_000L).coerceAtLeast(1L)
                                store.upsertPeer(peer.copy(latencyMs = rtt, lastSeenMs = System.currentTimeMillis(), online = true))
                            }
                        }
                    }
                }
                store.pruneStalePeers(20_000L)
                delay(4000)
            }
        }
    }

    private fun broadcastPresence(msgType: String) {
        val s = store.getSettings()
        if (s.status == "offline" && msgType != "bye") return
        val pktJson = JSONObject()
            .put("msg_type", msgType)
            .put("device_id", s.deviceId)
            .put("alias", s.alias)
            .put("device_type", s.deviceType)
            .put("port", boundPort)
            .put("fingerprint", tls.fingerprintHex)
            .put("protocol_version", 1)
            .put("status", s.status)
            .put("status_message", s.statusMessage)
            .put("os", s.os)
        val payload = pktJson.toString().toByteArray(Charsets.UTF_8)

        val targets = linkedSetOf("255.255.255.255", MULTICAST_ADDR)
        runCatching {
            val ifaces = NetworkInterface.getNetworkInterfaces()
            while (ifaces.hasMoreElements()) {
                val iface = ifaces.nextElement()
                if (!iface.isUp || iface.isLoopback) continue
                iface.interfaceAddresses.forEach { ia ->
                    ia.broadcast?.hostAddress?.let { targets.add(it) }
                }
            }
        }

        DatagramSocket().use { sock ->
            sock.broadcast = true
            targets.forEach { target ->
                runCatching {
                    val addr = InetAddress.getByName(target)
                    sock.send(DatagramPacket(payload, payload.size, addr, DISCOVERY_PORT))
                }
            }
        }
    }

    private fun probeActiveSubnets() {
        val ips = mutableListOf<String>()
        runCatching {
            val ifaces = NetworkInterface.getNetworkInterfaces()
            while (ifaces.hasMoreElements()) {
                val iface = ifaces.nextElement()
                if (!iface.isUp || iface.isLoopback) continue
                iface.interfaceAddresses.forEach { ia ->
                    val addr = ia.address
                    if (addr is Inet4Address) ips.add(addr.hostAddress ?: "")
                }
            }
        }
        val selfIps = ips.toSet()
        ips.firstOrNull { it.isNotBlank() }?.let { myIp ->
            val parts = myIp.split('.')
            if (parts.size == 4) {
                val prefix = "${parts[0]}.${parts[1]}.${parts[2]}"
                val sem = Semaphore(24)
                for (host in 1..254) {
                    val candidate = "$prefix.$host"
                    if (selfIps.contains(candidate)) continue
                    scope.launch {
                        sem.acquire()
                        try {
                            if (XentCrypto.tcpReachable(candidate, 53317, 250)) {
                                addPeerByIp("$candidate:53317")
                            }
                        } finally {
                            sem.release()
                        }
                    }
                }
            }
        }
    }

    // ---- Chat Operations ----

    fun sendMessage(peerId: String, content: String, quote: String? = null) {
        val trimmed = content.trim()
        if (trimmed.isEmpty()) return
        val me = store.getSettings()
        val peer = store.getPeer(peerId)
        val now = System.currentTimeMillis()
        val msgId = UUID.randomUUID().toString()
        val msg = ChatMessage(
            id = msgId,
            threadPeerId = peerId,
            senderId = me.deviceId,
            senderName = me.alias,
            receiverId = peerId,
            receiverName = peer?.alias ?: "Device",
            content = trimmed,
            status = if (peer?.online == true) "Sending" else "Queued",
            createdAt = now,
            isIncoming = false,
            isRead = true,
            quote = quote
        )
        store.saveOutgoingMessage(msg)
        if (peer != null && peer.online) {
            scope.launch { deliverChatMessage(msg, peer) }
        }
    }

    fun retryMessage(msg: ChatMessage) {
        val peer = store.getPeer(msg.threadPeerId) ?: return
        scope.launch { deliverChatMessage(msg, peer) }
    }

    private fun deliverChatMessage(msg: ChatMessage, peer: PeerInfo) {
        val body = JSONObject()
            .put("id", msg.id)
            .put("sender_id", msg.senderId)
            .put("sender_name", msg.senderName)
            .put("receiver_id", msg.receiverId)
            .put("receiver_name", msg.receiverName)
            .put("content", msg.content)
            .put("created_at", msg.createdAt)
            .apply { if (!msg.quote.isNullOrBlank()) put("quote", msg.quote) }
        val ok = runCatching {
            val (code, _) = XentCrypto.postJson(peer.ip, peer.port, "/api/message", body.toString(), tls.clientContext)
            code in 200..299
        }.getOrDefault(false)
        if (ok) {
            store.markMessageDelivered(msg.id, XentFormat.formatTime(System.currentTimeMillis()))
        } else {
            store.markMessageFailed(msg.id)
        }
    }

    fun sendTyping(peerId: String) {
        val peer = store.getPeer(peerId) ?: return
        if (!peer.online) return
        val me = store.getSettings()
        scope.launch {
            runCatching {
                val body = JSONObject().put("sender_id", me.deviceId).put("sender_name", me.alias)
                XentCrypto.postJson(peer.ip, peer.port, "/api/message/typing", body.toString(), tls.clientContext, 1200, 1500)
            }
        }
    }

    fun markThreadRead(peerId: String) {
        val updated = store.markThreadIncomingRead(peerId)
        if (updated <= 0) return
        val peer = store.getPeer(peerId) ?: return
        val me = store.getSettings()
        scope.launch {
            runCatching {
                val body = JSONObject().put("peer_id", me.deviceId)
                XentCrypto.postJson(peer.ip, peer.port, "/api/message/read", body.toString(), tls.clientContext)
            }
        }
    }

    fun editMessage(peerId: String, messageId: String, newContent: String) {
        val trimmed = newContent.trim()
        if (trimmed.isEmpty()) return
        store.applyEdit(messageId, trimmed)
        val peer = store.getPeer(peerId) ?: return
        scope.launch {
            runCatching {
                val body = JSONObject().put("message_id", messageId).put("new_content", trimmed)
                XentCrypto.postJson(peer.ip, peer.port, "/api/message/edit", body.toString(), tls.clientContext)
            }
        }
    }

    fun deleteMessage(peerId: String, messageId: String) {
        store.applyDelete(messageId)
        val peer = store.getPeer(peerId) ?: return
        scope.launch {
            runCatching {
                val body = JSONObject().put("message_id", messageId)
                XentCrypto.postJson(peer.ip, peer.port, "/api/message/delete", body.toString(), tls.clientContext)
            }
        }
    }

    fun toggleReaction(peerId: String, messageId: String, emoji: String) {
        val me = store.getSettings()
        store.toggleReaction(messageId, me.alias, emoji)
        val peer = store.getPeer(peerId) ?: return
        scope.launch {
            runCatching {
                val body = JSONObject()
                    .put("message_id", messageId)
                    .put("user_name", me.alias)
                    .put("emoji", emoji)
                XentCrypto.postJson(peer.ip, peer.port, "/api/message/reaction", body.toString(), tls.clientContext)
            }
        }
    }

    private fun startQueueFlusherLoop() {
        scope.launch {
            while (started.get()) {
                delay(5000)
                val queued = store.getQueuedMessages()
                for (m in queued) {
                    val peer = store.getPeer(m.threadPeerId)
                    if (peer != null && peer.online) {
                        deliverChatMessage(m, peer)
                    }
                }
            }
        }
    }

    // ---- File Transfer Operations ----

    fun resolveUriMetadata(uri: Uri): QueuedUploadSource? {
        val resolver = context?.contentResolver ?: return null
        var name = "file"
        var size = -1L
        resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)
            ?.use { cursor ->
                if (cursor.moveToFirst()) {
                    val nIdx = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                    val sIdx = cursor.getColumnIndex(OpenableColumns.SIZE)
                    if (nIdx >= 0) name = cursor.getString(nIdx) ?: name
                    if (sIdx >= 0 && !cursor.isNull(sIdx)) size = cursor.getLong(sIdx)
                }
            }
        if (size < 0L) {
            size = runCatching {
                resolver.openAssetFileDescriptor(uri, "r")?.use { it.length } ?: -1L
            }.getOrDefault(-1L)
        }
        return QueuedUploadSource(
            id = UUID.randomUUID().toString(),
            name = name,
            size = size.coerceAtLeast(0L),
            uriString = uri.toString()
        )
    }

    suspend fun sendFileRequest(peerId: String, source: QueuedUploadSource): Result<TransferItem> =
        kotlinx.coroutines.withContext(Dispatchers.IO) {
            runCatching {
                val peer = store.getPeer(peerId) ?: error("Peer not found")
                val me = store.getSettings()
                val transferId = UUID.randomUUID().toString()
                val item = TransferItem(
                    transferId = transferId,
                    fileName = source.name,
                    fileSize = source.size,
                    transferredBytes = 0L,
                    status = TransferStatus.PENDING,
                    isIncoming = false,
                    senderId = me.deviceId,
                    senderName = me.alias,
                    receiverId = peer.deviceId,
                    receiverName = peer.alias,
                    peerIp = peer.ip,
                    peerPort = peer.port,
                    filePath = source.localFile?.absolutePath,
                    sourceUri = source.uriString
                )
                store.putTransfer(item)
                uploadControls[transferId] = UploadControl(source = source)

                val reqJson = JSONObject()
                    .put("transfer_id", transferId)
                    .put("sender_id", me.deviceId)
                    .put("sender_name", me.alias)
                    .put("receiver_id", peer.deviceId)
                    .put("receiver_name", peer.alias)
                    .put("file_name", source.name)
                    .put("file_size", source.size)
                    .put("sender_port", boundPort)

                val (code, body) = XentCrypto.postJson(
                    peer.ip, peer.port, "/api/files/request", reqJson.toString(), tls.clientContext
                )
                if (code !in 200..299) {
                    val failed = store.updateTransfer(transferId) {
                        it.copy(status = TransferStatus.FAILED, error = "HTTP $code")
                    } ?: item.copy(status = TransferStatus.FAILED)
                    return@runCatching failed
                }
                val respJson = JSONObject(body)
                if (respJson.optBoolean("auto_declined", false)) {
                    return@runCatching store.updateTransfer(transferId) {
                        it.copy(status = TransferStatus.DECLINED)
                    } ?: item.copy(status = TransferStatus.DECLINED)
                }
                if (respJson.optBoolean("auto_accepted", false)) {
                    startUploadAsync(transferId)
                }
                store.getTransfer(transferId) ?: item
            }
        }

    fun respondToOffer(transferId: String, accept: Boolean) {
        val item = store.getTransfer(transferId) ?: return
        scope.launch {
            if (accept) {
                val reserved = XentFormat.reserveUniqueFile(downloadDir, item.fileName)
                synchronized(receiveLock) {
                    receiveFiles[transferId] = RandomAccessFile(reserved, "rw")
                }
                val doneZero = item.fileSize == 0L
                store.updateTransfer(transferId) {
                    it.copy(
                        filePath = reserved.absolutePath,
                        status = if (doneZero) TransferStatus.COMPLETED else TransferStatus.TRANSFERRING
                    )
                }
                if (doneZero) closeReceiveFile(transferId)
            } else {
                store.updateTransfer(transferId) { it.copy(status = TransferStatus.DECLINED) }
            }

            if (item.peerIp.isNotBlank()) {
                runCatching {
                    val body = JSONObject().put("transfer_id", transferId).put("accepted", accept)
                    XentCrypto.postJson(item.peerIp, item.peerPort, "/api/files/response", body.toString(), tls.clientContext)
                }
            }
        }
    }

    fun pauseTransfer(transferId: String) {
        uploadControls[transferId]?.paused?.set(true)
        val updated = store.updateTransfer(transferId) {
            if (it.status == TransferStatus.TRANSFERRING || it.status == TransferStatus.PENDING) {
                it.copy(status = TransferStatus.PAUSED, speedMbps = 0.0)
            } else it
        } ?: return
        if (updated.peerIp.isNotBlank()) {
            scope.launch {
                runCatching {
                    XentCrypto.postJson(updated.peerIp, updated.peerPort, "/api/files/pause/$transferId", "{}", tls.clientContext)
                }
            }
        }
    }

    fun resumeTransfer(transferId: String) {
        uploadControls[transferId]?.paused?.set(false)
        val updated = store.updateTransfer(transferId) {
            if (it.status == TransferStatus.PAUSED) {
                it.copy(status = TransferStatus.TRANSFERRING)
            } else it
        } ?: return
        if (updated.peerIp.isNotBlank()) {
            scope.launch {
                runCatching {
                    XentCrypto.postJson(updated.peerIp, updated.peerPort, "/api/files/resume/$transferId", "{}", tls.clientContext)
                }
            }
        }
    }

    fun cancelTransfer(transferId: String) {
        uploadControls[transferId]?.cancelled?.set(true)
        closeReceiveFile(transferId)
        val updated = store.updateTransfer(transferId) {
            if (!it.status.isTerminal()) it.copy(status = TransferStatus.CANCELLED, speedMbps = 0.0) else it
        } ?: return
        if (updated.peerIp.isNotBlank()) {
            scope.launch {
                runCatching {
                    XentCrypto.postJson(updated.peerIp, updated.peerPort, "/api/files/cancel/$transferId", "{}", tls.clientContext)
                }
            }
        }
    }

    private fun startUploadAsync(transferId: String) {
        val ctrl = uploadControls[transferId] ?: return
        if (!ctrl.started.compareAndSet(false, true)) return
        val item = store.getTransfer(transferId) ?: return
        if (item.fileSize == 0L) {
            store.updateTransfer(transferId) { it.copy(status = TransferStatus.COMPLETED, transferredBytes = 0L) }
            uploadControls.remove(transferId)
            return
        }
        store.updateTransfer(transferId) { it.copy(status = TransferStatus.TRANSFERRING) }

        scope.launch {
            var offset = 0L
            val buf = ByteArray(CHUNK_SIZE)
            val startTime = System.currentTimeMillis()
            var stream: InputStream? = null

            try {
                stream = openSourceStream(ctrl.source)
                    ?: throw IllegalStateException("Cannot open selected file")
                uploadPermits.acquire()
                var holdingPermit = true
                try {
                    while (offset < item.fileSize) {
                        if (ctrl.cancelled.get()) {
                            store.updateTransfer(transferId) {
                                it.copy(status = TransferStatus.CANCELLED, speedMbps = 0.0)
                            }
                            return@launch
                        }
                        if (ctrl.paused.get()) {
                            if (holdingPermit) {
                                uploadPermits.release()
                                holdingPermit = false
                            }
                            store.updateTransfer(transferId) {
                                it.copy(status = TransferStatus.PAUSED, speedMbps = 0.0)
                            }
                            while (ctrl.paused.get() && !ctrl.cancelled.get()) {
                                delay(120)
                            }
                            if (ctrl.cancelled.get()) continue
                            uploadPermits.acquire()
                            holdingPermit = true
                            store.updateTransfer(transferId) { it.copy(status = TransferStatus.TRANSFERRING) }
                        }

                        var totalRead = 0
                        while (totalRead < buf.size) {
                            val r = stream.read(buf, totalRead, buf.size - totalRead)
                            if (r <= 0) break
                            totalRead += r
                        }
                        if (totalRead <= 0) break

                        val chunk = if (totalRead == buf.size) buf else buf.copyOf(totalRead)
                        sendChunkWithRetry(item.peerIp, item.peerPort, transferId, offset, chunk)
                        offset += totalRead
                        val elapsedSec = ((System.currentTimeMillis() - startTime).coerceAtLeast(100L)) / 1000.0
                        val speed = (offset / 1_048_576.0) / elapsedSec
                        val done = offset >= item.fileSize
                        store.updateTransfer(transferId) {
                            it.copy(
                                transferredBytes = offset,
                                speedMbps = if (done) 0.0 else speed,
                                status = if (done) TransferStatus.COMPLETED else if (ctrl.paused.get()) TransferStatus.PAUSED else TransferStatus.TRANSFERRING
                            )
                        }
                    }
                } finally {
                    if (holdingPermit) uploadPermits.release()
                }
            } catch (e: Exception) {
                store.updateTransfer(transferId) {
                    it.copy(status = TransferStatus.FAILED, speedMbps = 0.0, error = e.message ?: "Upload failed")
                }
            } finally {
                runCatching { stream?.close() }
                uploadControls.remove(transferId)
            }
        }
    }

    private fun openSourceStream(source: QueuedUploadSource): InputStream? {
        source.localFile?.let { f -> if (f.exists()) return FileInputStream(f) }
        val uriStr = source.uriString ?: return null
        val resolver: ContentResolver = context?.contentResolver ?: return null
        return resolver.openInputStream(Uri.parse(uriStr))
    }

    private fun sendChunkWithRetry(ip: String, port: Int, transferId: String, offset: Long, data: ByteArray) {
        var lastErr: Exception? = null
        for (attempt in 1..3) {
            try {
                val conn = XentCrypto.openHttps(
                    "https://$ip:$port/api/files/chunk/$transferId?offset=$offset",
                    tls.clientContext,
                    4000,
                    30_000
                )
                try {
                    conn.requestMethod = "POST"
                    conn.doOutput = true
                    conn.setRequestProperty("Content-Type", "application/octet-stream")
                    conn.setFixedLengthStreamingMode(data.size)
                    conn.outputStream.use { it.write(data) }
                    val code = conn.responseCode
                    if (code in 200..299) return
                    lastErr = IllegalStateException("Chunk HTTP $code")
                } finally {
                    conn.disconnect()
                }
            } catch (e: Exception) {
                lastErr = e
            }
            Thread.sleep(300L * attempt)
        }
        throw lastErr ?: IllegalStateException("Chunk upload failed")
    }
}
