package com.xent.app

import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import kotlin.math.max

enum class TransferStatus {
    PENDING,
    TRANSFERRING,
    PAUSED,
    COMPLETED,
    FAILED,
    DECLINED,
    CANCELLED;

    fun isTerminal(): Boolean = when (this) {
        COMPLETED, FAILED, DECLINED, CANCELLED -> true
        else -> false
    }

    fun isActive(): Boolean = when (this) {
        PENDING, TRANSFERRING, PAUSED -> true
        else -> false
    }
}

data class XentSettings(
    val deviceId: String,
    val alias: String,
    val deviceType: String = "mobile",
    val os: String = "android",
    val port: Int = 53317,
    val status: String = "available",
    val statusMessage: String = "",
    val theme: String = "system", // "light" | "dark" | "system"
    val autoAcceptFavorites: Boolean = true,
    val discoveryEnabled: Boolean = true,
    val soundEnabled: Boolean = true,
    val favoritePeers: List<String> = emptyList(),
    val blockedPeers: List<String> = emptyList(),
    val peerNicknames: Map<String, String> = emptyMap(),
    val manualPeers: List<String> = emptyList()
) {
    fun toJson(): JSONObject = JSONObject().apply {
        put("device_id", deviceId)
        put("alias", alias)
        put("device_type", deviceType)
        put("os", os)
        put("port", port)
        put("status", status)
        put("status_message", statusMessage)
        put("theme", theme)
        put("auto_accept_favorites", autoAcceptFavorites)
        put("discovery_enabled", discoveryEnabled)
        put("sound_enabled", soundEnabled)
        put("favorite_peers", JSONArray(favoritePeers))
        put("blocked_peers", JSONArray(blockedPeers))
        put("peer_nicknames", JSONObject(peerNicknames))
        put("manual_peers", JSONArray(manualPeers))
    }

    companion object {
        fun fromJson(obj: JSONObject, defaultDeviceId: String, defaultAlias: String): XentSettings {
            val favs = mutableListOf<String>()
            obj.optJSONArray("favorite_peers")?.let { arr ->
                for (i in 0 until arr.length()) favs.add(arr.optString(i))
            }
            val blocked = mutableListOf<String>()
            obj.optJSONArray("blocked_peers")?.let { arr ->
                for (i in 0 until arr.length()) blocked.add(arr.optString(i))
            }
            val manual = mutableListOf<String>()
            obj.optJSONArray("manual_peers")?.let { arr ->
                for (i in 0 until arr.length()) manual.add(arr.optString(i))
            }
            val nicks = mutableMapOf<String, String>()
            obj.optJSONObject("peer_nicknames")?.let { nickObj ->
                nickObj.keys().forEach { k -> nicks[k] = nickObj.optString(k) }
            }
            return XentSettings(
                deviceId = obj.optString("device_id", defaultDeviceId).ifBlank { defaultDeviceId },
                alias = obj.optString("alias", defaultAlias).ifBlank { defaultAlias },
                deviceType = obj.optString("device_type", "mobile"),
                os = obj.optString("os", "android"),
                port = obj.optInt("port", 53317).let { if (it in 1024..65535) it else 53317 },
                status = obj.optString("status", "available"),
                statusMessage = obj.optString("status_message", ""),
                theme = obj.optString("theme", "system"),
                autoAcceptFavorites = obj.optBoolean("auto_accept_favorites", true),
                discoveryEnabled = obj.optBoolean("discovery_enabled", true),
                soundEnabled = obj.optBoolean("sound_enabled", true),
                favoritePeers = favs.distinct(),
                blockedPeers = blocked.distinct(),
                peerNicknames = nicks,
                manualPeers = manual.distinct()
            )
        }
    }
}

data class PeerInfo(
    val deviceId: String,
    val alias: String,
    val deviceType: String = "desktop",
    val os: String? = null,
    val ip: String = "",
    val port: Int = 53317,
    val fingerprint: String = "",
    val status: String = "available",
    val statusMessage: String = "",
    val lastSeenMs: Long = System.currentTimeMillis(),
    val online: Boolean = true
) {
    fun toJson(): JSONObject = JSONObject().apply {
        put("device_id", deviceId)
        put("alias", alias)
        put("device_type", deviceType)
        put("os", os ?: JSONObject.NULL)
        put("ip", ip)
        put("port", port)
        put("fingerprint", fingerprint)
        put("status", status)
        put("status_message", statusMessage)
        put("last_seen_ms", lastSeenMs)
        put("online", online)
    }

    companion object {
        fun fromJson(obj: JSONObject): PeerInfo = PeerInfo(
            deviceId = obj.optString("device_id"),
            alias = obj.optString("alias", "Device"),
            deviceType = obj.optString("device_type", "desktop"),
            os = obj.optString("os", "").ifBlank { null },
            ip = obj.optString("ip", ""),
            port = obj.optInt("port", 53317),
            fingerprint = obj.optString("fingerprint", ""),
            status = obj.optString("status", "available"),
            statusMessage = obj.optString("status_message", ""),
            lastSeenMs = obj.optLong("last_seen_ms", System.currentTimeMillis()),
            online = obj.optBoolean("online", false)
        )
    }
}

data class ChatMessage(
    val id: String,
    val threadPeerId: String,
    val senderId: String,
    val senderName: String,
    val receiverId: String,
    val receiverName: String,
    val content: String,
    val status: String, // "Queued" | "Sending" | "Delivered, ..." | "Read, ..." | "Failed"
    val createdAt: Long,
    val isIncoming: Boolean,
    val isRead: Boolean,
    val quote: String? = null,
    val isEdited: Boolean = false,
    val isDeleted: Boolean = false,
    val reactions: Map<String, List<String>> = emptyMap()
) {
    fun toJson(): JSONObject = JSONObject().apply {
        put("id", id)
        put("thread_peer_id", threadPeerId)
        put("sender_id", senderId)
        put("sender_name", senderName)
        put("receiver_id", receiverId)
        put("receiver_name", receiverName)
        put("content", content)
        put("status", status)
        put("created_at", createdAt)
        put("is_incoming", isIncoming)
        put("is_read", isRead)
        put("quote", quote ?: JSONObject.NULL)
        put("is_edited", isEdited)
        put("is_deleted", isDeleted)
        val reactObj = JSONObject()
        reactions.forEach { (emoji, users) -> reactObj.put(emoji, JSONArray(users)) }
        put("reactions", reactObj)
    }

    companion object {
        fun fromJson(obj: JSONObject): ChatMessage {
            val reactMap = linkedMapOf<String, MutableList<String>>()
            obj.optJSONObject("reactions")?.let { r ->
                r.keys().forEach { emoji ->
                    val list = mutableListOf<String>()
                    val arr = r.optJSONArray(emoji)
                    if (arr != null) {
                        for (i in 0 until arr.length()) list.add(arr.optString(i))
                    }
                    if (list.isNotEmpty()) reactMap[emoji] = list
                }
            }
            return ChatMessage(
                id = obj.optString("id"),
                threadPeerId = obj.optString("thread_peer_id"),
                senderId = obj.optString("sender_id"),
                senderName = obj.optString("sender_name"),
                receiverId = obj.optString("receiver_id"),
                receiverName = obj.optString("receiver_name"),
                content = obj.optString("content"),
                status = obj.optString("status", "Delivered"),
                createdAt = obj.optLong("created_at", System.currentTimeMillis()),
                isIncoming = obj.optBoolean("is_incoming", false),
                isRead = obj.optBoolean("is_read", true),
                quote = if (obj.isNull("quote")) null else obj.optString("quote").ifBlank { null },
                isEdited = obj.optBoolean("is_edited", false),
                isDeleted = obj.optBoolean("is_deleted", false),
                reactions = reactMap
            )
        }
    }
}

data class TransferItem(
    val transferId: String,
    val fileName: String,
    val fileSize: Long,
    val transferredBytes: Long = 0L,
    val speedMbps: Double = 0.0,
    val status: TransferStatus = TransferStatus.PENDING,
    val isIncoming: Boolean,
    val senderId: String,
    val senderName: String,
    val receiverId: String,
    val receiverName: String,
    val peerIp: String = "",
    val peerPort: Int = 53317,
    val filePath: String? = null,
    val sourceUri: String? = null,
    val error: String? = null,
    val createdAt: Long = System.currentTimeMillis()
) {
    val progressFraction: Float
        get() = if (fileSize <= 0L) {
            if (status == TransferStatus.COMPLETED) 1f else 0f
        } else {
            (transferredBytes.toDouble() / fileSize.toDouble()).toFloat().coerceIn(0f, 1f)
        }

    fun toJson(): JSONObject = JSONObject().apply {
        put("transfer_id", transferId)
        put("file_name", fileName)
        put("file_size", fileSize)
        put("transferred_bytes", transferredBytes)
        put("speed_mbps", speedMbps)
        put("status", status.name)
        put("is_incoming", isIncoming)
        put("sender_id", senderId)
        put("sender_name", senderName)
        put("receiver_id", receiverId)
        put("receiver_name", receiverName)
        put("peer_ip", peerIp)
        put("peer_port", peerPort)
        put("file_path", filePath ?: JSONObject.NULL)
        put("error", error ?: JSONObject.NULL)
        put("created_at", createdAt)
    }

    companion object {
        fun fromJson(obj: JSONObject): TransferItem {
            val statusStr = obj.optString("status", "FAILED")
            val parsedStatus = runCatching { TransferStatus.valueOf(statusStr) }
                .getOrDefault(TransferStatus.FAILED)
            val normalizedStatus = if (!parsedStatus.isTerminal()) TransferStatus.FAILED else parsedStatus
            return TransferItem(
                transferId = obj.optString("transfer_id"),
                fileName = obj.optString("file_name", "file"),
                fileSize = obj.optLong("file_size", 0L),
                transferredBytes = obj.optLong("transferred_bytes", 0L),
                speedMbps = obj.optDouble("speed_mbps", 0.0),
                status = normalizedStatus,
                isIncoming = obj.optBoolean("is_incoming", false),
                senderId = obj.optString("sender_id"),
                senderName = obj.optString("sender_name"),
                receiverId = obj.optString("receiver_id"),
                receiverName = obj.optString("receiver_name"),
                peerIp = obj.optString("peer_ip", ""),
                peerPort = obj.optInt("peer_port", 53317),
                filePath = if (obj.isNull("file_path")) null else obj.optString("file_path").ifBlank { null },
                error = if (obj.isNull("error")) null else obj.optString("error").ifBlank { null },
                createdAt = obj.optLong("created_at", System.currentTimeMillis())
            )
        }
    }
}

data class QueuedUploadSource(
    val id: String,
    val name: String,
    val size: Long,
    val uriString: String? = null,
    val localFile: File? = null
)

object XentFormat {
    private val reservedNames = setOf(
        "CON", "PRN", "AUX", "NUL",
        "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
        "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9"
    )

    fun sanitizeFileName(raw: String): String {
        val lastSlash = max(raw.lastIndexOf('/'), raw.lastIndexOf('\\'))
        val tail = if (lastSlash >= 0) raw.substring(lastSlash + 1) else raw
        val cleaned = buildString {
            for (ch in tail) {
                if (ch.code < 32 || ch in listOf('<', '>', ':', '"', '/', '\\', '|', '?', '*')) {
                    append('_')
                } else {
                    append(ch)
                }
            }
        }.trim().trimStart('.')
        if (cleaned.isEmpty() || cleaned == "." || cleaned == "..") {
            return "received_file"
        }
        val dot = cleaned.indexOf('.')
        val stem = if (dot >= 0) cleaned.substring(0, dot) else cleaned
        return if (reservedNames.contains(stem.uppercase(Locale.US))) "_$cleaned" else cleaned
    }

    fun reserveUniqueFile(dir: File, rawName: String): File {
        dir.mkdirs()
        val safe = sanitizeFileName(rawName)
        val dot = safe.lastIndexOf('.')
        val stem = if (dot > 0) safe.substring(0, dot) else safe
        val ext = if (dot > 0) safe.substring(dot) else ""

        for (idx in 0..9999) {
            val candidateName = if (idx == 0) safe else "$stem ($idx)$ext"
            val candidate = File(dir, candidateName)
            if (!candidate.exists() && candidate.createNewFile()) {
                return candidate
            }
        }
        return File.createTempFile("xent_", "_$safe", dir)
    }

    fun formatBytes(bytes: Long): String {
        if (bytes < 1024) return "$bytes B"
        val kb = bytes / 1024.0
        if (kb < 1024.0) return String.format(Locale.US, "%.1f KB", kb)
        val mb = kb / 1024.0
        if (mb < 1024.0) return String.format(Locale.US, "%.1f MB", mb)
        return String.format(Locale.US, "%.2f GB", mb / 1024.0)
    }

    fun formatTime(ms: Long): String {
        val fmt = SimpleDateFormat("h:mm a", Locale.getDefault())
        return fmt.format(Date(ms))
    }

    fun formatDateHeader(ms: Long): String {
        val fmt = SimpleDateFormat("MMM d, yyyy", Locale.getDefault())
        return fmt.format(Date(ms))
    }
}
