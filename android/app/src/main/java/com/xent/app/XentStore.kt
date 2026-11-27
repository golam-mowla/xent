package com.xent.app

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.UUID

class XentStore(
    val dataDir: File,
    defaultAlias: String
) {
    private val lock = Any()
    private val settingsFile = File(dataDir, "settings.json")
    private val peersFile = File(dataDir, "peers.json")
    private val messagesFile = File(dataDir, "messages.json")
    private val transfersFile = File(dataDir, "transfers.json")

    private var _settingsVal: XentSettings
    private val peersMap = linkedMapOf<String, PeerInfo>()
    private val messagesList = mutableListOf<ChatMessage>()
    private val transfersMap = linkedMapOf<String, TransferItem>()
    private val pendingOfferIds = linkedSetOf<String>()

    private val _settingsFlow: MutableStateFlow<XentSettings>
    val settingsFlow: StateFlow<XentSettings>

    private val _peersFlow = MutableStateFlow<List<PeerInfo>>(emptyList())
    val peersFlow: StateFlow<List<PeerInfo>> = _peersFlow.asStateFlow()

    private val _messagesFlow = MutableStateFlow<List<ChatMessage>>(emptyList())
    val messagesFlow: StateFlow<List<ChatMessage>> = _messagesFlow.asStateFlow()

    private val _transfersFlow = MutableStateFlow<List<TransferItem>>(emptyList())
    val transfersFlow: StateFlow<List<TransferItem>> = _transfersFlow.asStateFlow()

    private val _incomingOfferFlow = MutableStateFlow<TransferItem?>(null)
    val incomingOfferFlow: StateFlow<TransferItem?> = _incomingOfferFlow.asStateFlow()

    private val _typingFlow = MutableStateFlow<Map<String, Long>>(emptyMap())
    val typingFlow: StateFlow<Map<String, Long>> = _typingFlow.asStateFlow()

    init {
        dataDir.mkdirs()
        val freshId = UUID.randomUUID().toString()
        _settingsVal = if (settingsFile.exists()) {
            runCatching {
                XentSettings.fromJson(JSONObject(settingsFile.readText()), freshId, defaultAlias)
            }.getOrElse {
                XentSettings(deviceId = freshId, alias = defaultAlias)
            }
        } else {
            XentSettings(deviceId = freshId, alias = defaultAlias)
        }
        saveSettingsLocked()

        if (peersFile.exists()) {
            runCatching {
                val arr = JSONArray(peersFile.readText())
                for (i in 0 until arr.length()) {
                    val p = PeerInfo.fromJson(arr.getJSONObject(i)).copy(online = false)
                    if (p.deviceId.isNotBlank()) peersMap[p.deviceId] = p
                }
            }
        }

        if (messagesFile.exists()) {
            runCatching {
                val arr = JSONArray(messagesFile.readText())
                for (i in 0 until arr.length()) {
                    val m = ChatMessage.fromJson(arr.getJSONObject(i))
                    if (m.id.isNotBlank()) messagesList.add(m)
                }
            }
        }

        if (transfersFile.exists()) {
            runCatching {
                val arr = JSONArray(transfersFile.readText())
                for (i in 0 until arr.length()) {
                    val t = TransferItem.fromJson(arr.getJSONObject(i))
                    if (t.transferId.isNotBlank()) transfersMap[t.transferId] = t
                }
            }
        }

        _settingsFlow = MutableStateFlow(_settingsVal)
        settingsFlow = _settingsFlow.asStateFlow()
        emitPeersLocked()
        emitMessagesLocked()
        emitTransfersLocked()
    }

    fun getSettings(): XentSettings = synchronized(lock) { _settingsVal }

    fun updateSettings(transform: (XentSettings) -> XentSettings): XentSettings = synchronized(lock) {
        _settingsVal = transform(_settingsVal)
        saveSettingsLocked()
        _settingsFlow.value = _settingsVal
        _settingsVal
    }

    fun isFavorite(deviceId: String): Boolean = synchronized(lock) {
        _settingsVal.favoritePeers.contains(deviceId)
    }

    fun isBlocked(deviceId: String): Boolean = synchronized(lock) {
        _settingsVal.blockedPeers.contains(deviceId)
    }

    fun getPeers(): List<PeerInfo> = synchronized(lock) {
        peersMap.values.sortedWith(compareByDescending<PeerInfo> { it.online }.thenByDescending { it.lastSeenMs })
    }

    fun getPeer(deviceId: String): PeerInfo? = synchronized(lock) { peersMap[deviceId] }

    fun findPeerByIp(ip: String): PeerInfo? = synchronized(lock) {
        peersMap.values.firstOrNull { it.ip == ip }
    }

    fun upsertPeer(peer: PeerInfo) = synchronized(lock) {
        if (peer.deviceId.isBlank() || peer.deviceId == _settingsVal.deviceId) return@synchronized
        val prev = peersMap[peer.deviceId]
        val merged = peer.copy(
            latencyMs = peer.latencyMs ?: prev?.latencyMs,
            os = peer.os ?: prev?.os,
            fingerprint = peer.fingerprint.ifBlank { prev?.fingerprint ?: "" }
        )
        peersMap[peer.deviceId] = merged
        savePeersLocked()
        emitPeersLocked()
    }

    fun markPeerOffline(deviceId: String) = synchronized(lock) {
        val existing = peersMap[deviceId] ?: return@synchronized
        if (existing.online) {
            peersMap[deviceId] = existing.copy(online = false)
            savePeersLocked()
            emitPeersLocked()
        }
    }

    fun pruneStalePeers(staleAfterMs: Long = 20_000L) = synchronized(lock) {
        val now = System.currentTimeMillis()
        var changed = false
        for ((id, p) in peersMap) {
            if (p.online && now - p.lastSeenMs > staleAfterMs) {
                peersMap[id] = p.copy(online = false)
                changed = true
            }
        }
        if (changed) {
            savePeersLocked()
            emitPeersLocked()
        }
    }

    fun setPeerTyping(peerId: String) {
        val cur = _typingFlow.value.toMutableMap()
        cur[peerId] = System.currentTimeMillis()
        _typingFlow.value = cur
    }

    fun saveOutgoingMessage(msg: ChatMessage) = synchronized(lock) {
        messagesList.removeAll { it.id == msg.id }
        messagesList.add(msg)
        messagesList.sortBy { it.createdAt }
        saveMessagesLocked()
        emitMessagesLocked()
    }

    /** Deduplicated incoming message insert (returns true if new). */
    fun saveIncomingMessage(msg: ChatMessage): Boolean = synchronized(lock) {
        if (messagesList.any { it.id == msg.id }) return@synchronized false
        messagesList.add(msg)
        messagesList.sortBy { it.createdAt }
        saveMessagesLocked()
        emitMessagesLocked()
        true
    }

    fun getMessages(): List<ChatMessage> = synchronized(lock) { messagesList.toList() }

    fun getQueuedMessages(): List<ChatMessage> = synchronized(lock) {
        messagesList.filter { !it.isIncoming && (it.status == "Queued" || it.status == "Failed") }
    }

    fun markMessageDelivered(id: String, timeLabel: String) = synchronized(lock) {
        val idx = messagesList.indexOfFirst { it.id == id }
        if (idx >= 0) {
            messagesList[idx] = messagesList[idx].copy(status = "Delivered, $timeLabel")
            saveMessagesLocked()
            emitMessagesLocked()
        }
    }

    fun markMessageFailed(id: String) = synchronized(lock) {
        val idx = messagesList.indexOfFirst { it.id == id }
        if (idx >= 0) {
            messagesList[idx] = messagesList[idx].copy(status = "Failed")
            saveMessagesLocked()
            emitMessagesLocked()
        }
    }

    fun markThreadIncomingRead(peerId: String): Int = synchronized(lock) {
        var count = 0
        for (i in messagesList.indices) {
            val m = messagesList[i]
            if (m.threadPeerId == peerId && m.isIncoming && !m.isRead) {
                messagesList[i] = m.copy(isRead = true)
                count++
            }
        }
        if (count > 0) {
            saveMessagesLocked()
            emitMessagesLocked()
        }
        count
    }

    fun markOutgoingRead(peerId: String, timeLabel: String) = synchronized(lock) {
        var changed = false
        for (i in messagesList.indices) {
            val m = messagesList[i]
            if (m.threadPeerId == peerId && !m.isIncoming && !m.status.startsWith("Read")) {
                messagesList[i] = m.copy(status = "Read, $timeLabel")
                changed = true
            }
        }
        if (changed) {
            saveMessagesLocked()
            emitMessagesLocked()
        }
    }

    fun applyEdit(messageId: String, newContent: String) = synchronized(lock) {
        val idx = messagesList.indexOfFirst { it.id == messageId }
        if (idx >= 0) {
            messagesList[idx] = messagesList[idx].copy(content = newContent, isEdited = true)
            saveMessagesLocked()
            emitMessagesLocked()
        }
    }

    fun applyDelete(messageId: String) = synchronized(lock) {
        val idx = messagesList.indexOfFirst { it.id == messageId }
        if (idx >= 0) {
            messagesList[idx] = messagesList[idx].copy(
                content = "This message was deleted",
                isDeleted = true
            )
            saveMessagesLocked()
            emitMessagesLocked()
        }
    }

    fun toggleReaction(messageId: String, userName: String, emoji: String) = synchronized(lock) {
        val idx = messagesList.indexOfFirst { it.id == messageId }
        if (idx >= 0) {
            val m = messagesList[idx]
            val map = m.reactions.mapValues { it.value.toMutableList() }.toMutableMap()
            val users = map.getOrPut(emoji) { mutableListOf() }
            if (users.contains(userName)) {
                users.remove(userName)
                if (users.isEmpty()) map.remove(emoji)
            } else {
                users.add(userName)
            }
            messagesList[idx] = m.copy(reactions = map)
            saveMessagesLocked()
            emitMessagesLocked()
        }
    }

    fun putTransfer(item: TransferItem) = synchronized(lock) {
        transfersMap[item.transferId] = item
        if (item.isIncoming && item.status == TransferStatus.PENDING) {
            pendingOfferIds.add(item.transferId)
        } else {
            pendingOfferIds.remove(item.transferId)
        }
        saveTransfersLocked()
        emitTransfersLocked()
        updateIncomingOfferLocked()
    }

    fun getTransfer(transferId: String): TransferItem? = synchronized(lock) { transfersMap[transferId] }

    fun getTransfers(): List<TransferItem> = synchronized(lock) {
        transfersMap.values.sortedByDescending { it.createdAt }
    }

    fun updateTransfer(transferId: String, transform: (TransferItem) -> TransferItem): TransferItem? =
        synchronized(lock) {
            val cur = transfersMap[transferId] ?: return@synchronized null
            val next = transform(cur)
            transfersMap[transferId] = next
            if (next.status != TransferStatus.PENDING) {
                pendingOfferIds.remove(transferId)
            }
            saveTransfersLocked()
            emitTransfersLocked()
            updateIncomingOfferLocked()
            next
        }

    fun clearFinishedTransfers() = synchronized(lock) {
        transfersMap.entries.removeIf { it.value.status.isTerminal() }
        saveTransfersLocked()
        emitTransfersLocked()
    }

    private fun updateIncomingOfferLocked() {
        val offerId = pendingOfferIds.firstOrNull { id ->
            transfersMap[id]?.status == TransferStatus.PENDING
        }
        _incomingOfferFlow.value = offerId?.let { transfersMap[it] }
    }

    private fun emitPeersLocked() {
        _peersFlow.value = peersMap.values.sortedWith(
            compareByDescending<PeerInfo> { it.online }.thenByDescending { it.lastSeenMs }
        )
    }

    private fun emitMessagesLocked() {
        _messagesFlow.value = messagesList.toList()
    }

    private fun emitTransfersLocked() {
        _transfersFlow.value = transfersMap.values.sortedByDescending { it.createdAt }
    }

    private fun saveSettingsLocked() {
        runCatching { settingsFile.writeText(_settingsVal.toJson().toString(2)) }
    }

    private fun savePeersLocked() {
        runCatching {
            val arr = JSONArray()
            peersMap.values.forEach { arr.put(it.toJson()) }
            peersFile.writeText(arr.toString())
        }
    }

    private fun saveMessagesLocked() {
        runCatching {
            val arr = JSONArray()
            messagesList.takeLast(2000).forEach { arr.put(it.toJson()) }
            messagesFile.writeText(arr.toString())
        }
    }

    private fun saveTransfersLocked() {
        runCatching {
            val arr = JSONArray()
            transfersMap.values.take(300).forEach { arr.put(it.toJson()) }
            transfersFile.writeText(arr.toString())
        }
    }
}
