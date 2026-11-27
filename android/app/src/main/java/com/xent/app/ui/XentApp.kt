package com.xent.app.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Reply
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.AttachFile
import androidx.compose.material.icons.filled.Block
import androidx.compose.material.icons.filled.Cancel
import androidx.compose.material.icons.filled.ChatBubbleOutline
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Computer
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Devices
import androidx.compose.material.icons.filled.DoneAll
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.FolderOpen
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PhoneAndroid
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.filled.StarBorder
import androidx.compose.material.icons.filled.SwapVert
import androidx.compose.material.icons.filled.UploadFile
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.xent.app.AndroidUpdateInfo
import com.xent.app.BuildConfig
import com.xent.app.ChatMessage
import com.xent.app.PeerInfo
import com.xent.app.QueuedUploadSource
import com.xent.app.R
import com.xent.app.TransferItem
import com.xent.app.TransferStatus
import com.xent.app.XentEngine
import com.xent.app.XentFormat
import com.xent.app.XentSettings
import com.xent.app.XentUpdater
import kotlinx.coroutines.launch

enum class XentTab { MESSAGES, NEARBY, SEND_FILES, TRANSFERS }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun XentAppScreen(
    engine: XentEngine,
    onOpenReceivedFile: (String) -> String?
) {
    val settings by engine.store.settingsFlow.collectAsState()
    val peers by engine.store.peersFlow.collectAsState()
    val messages by engine.store.messagesFlow.collectAsState()
    val transfers by engine.store.transfersFlow.collectAsState()
    val incomingOffer by engine.store.incomingOfferFlow.collectAsState()
    val typingMap by engine.store.typingFlow.collectAsState()

    var currentTab by remember { mutableStateOf(XentTab.MESSAGES) }
    var activePeerId by remember { mutableStateOf<String?>(null) }
    var preselectedRecipientId by remember { mutableStateOf<String?>(null) }
    var queuedFiles by remember { mutableStateOf<List<QueuedUploadSource>>(emptyList()) }
    var showSettings by remember { mutableStateOf(false) }
    var showConnectIp by remember { mutableStateOf(false) }

    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    val unreadTotal = messages.count { it.isIncoming && !it.isRead }
    val activeTransfersCount = transfers.count { it.status.isActive() }

    val multiFilePicker = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.OpenMultipleDocuments()
    ) { uris ->
        if (uris.isNotEmpty()) {
            val resolved = uris.mapNotNull { engine.resolveUriMetadata(it) }
            queuedFiles = (queuedFiles + resolved).distinctBy { it.uriString ?: it.id }
            currentTab = XentTab.SEND_FILES
        }
    }

    val chatAttachPicker = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.OpenMultipleDocuments()
    ) { uris ->
        val targetPeerId = activePeerId
        if (uris.isNotEmpty() && targetPeerId != null) {
            val resolved = uris.mapNotNull { engine.resolveUriMetadata(it) }
            if (resolved.isNotEmpty()) {
                scope.launch {
                    var sentCount = 0
                    for (src in resolved) {
                        if (engine.sendFileRequest(targetPeerId, src).isSuccess) sentCount++
                    }
                    if (sentCount > 0) {
                        Toast.makeText(context, "Sending $sentCount file(s)", Toast.LENGTH_SHORT).show()
                    }
                }
            }
        }
    }

    XentTheme(settings.theme) {
        Scaffold(
            topBar = {
                if (activePeerId == null) {
                    TopAppBar(
                        title = {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Image(
                                    painter = painterResource(id = R.drawable.ic_xent_logo),
                                    contentDescription = "Xent",
                                    modifier = Modifier.size(28.dp).clip(RoundedCornerShape(6.dp))
                                )
                                Spacer(Modifier.width(10.dp))
                                Column {
                                    Text(
                                        text = when (currentTab) {
                                            XentTab.MESSAGES -> "Messages"
                                            XentTab.NEARBY -> "Nearby Devices"
                                            XentTab.SEND_FILES -> "Send Files"
                                            XentTab.TRANSFERS -> "Transfers"
                                        },
                                        fontWeight = FontWeight.SemiBold,
                                        fontSize = 18.sp
                                    )
                                    Text(
                                        text = "${settings.alias} • Port ${engine.boundPort}",
                                        style = MaterialTheme.typography.labelSmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant
                                    )
                                }
                            }
                        },
                        actions = {
                            if (currentTab == XentTab.NEARBY) {
                                IconButton(onClick = { showConnectIp = true }) {
                                    Icon(Icons.Default.Add, contentDescription = "Connect by IP")
                                }
                                IconButton(onClick = {
                                    engine.rescanNetwork()
                                    Toast.makeText(context, "Scanning LAN...", Toast.LENGTH_SHORT).show()
                                }) {
                                    Icon(Icons.Default.Refresh, contentDescription = "Rescan")
                                }
                            }
                            IconButton(onClick = { showSettings = true }) {
                                Icon(Icons.Default.Settings, contentDescription = "Settings")
                            }
                        },
                        colors = TopAppBarDefaults.topAppBarColors(
                            containerColor = MaterialTheme.colorScheme.surface
                        )
                    )
                }
            },
            bottomBar = {
                if (activePeerId == null) {
                    NavigationBar(containerColor = MaterialTheme.colorScheme.surface) {
                        NavigationBarItem(
                            selected = currentTab == XentTab.MESSAGES,
                            onClick = { currentTab = XentTab.MESSAGES },
                            icon = {
                                BadgedBox(badge = {
                                    if (unreadTotal > 0) Badge { Text("$unreadTotal") }
                                }) {
                                    Icon(Icons.Default.ChatBubbleOutline, contentDescription = "Messages")
                                }
                            },
                            label = { Text("Messages", maxLines = 1) }
                        )
                        NavigationBarItem(
                            selected = currentTab == XentTab.NEARBY,
                            onClick = { currentTab = XentTab.NEARBY },
                            icon = {
                                BadgedBox(badge = {
                                    val online = peers.count { it.online }
                                    if (online > 0) Badge { Text("$online") }
                                }) {
                                    Icon(Icons.Default.Devices, contentDescription = "Nearby")
                                }
                            },
                            label = { Text("Nearby", maxLines = 1) }
                        )
                        NavigationBarItem(
                            selected = currentTab == XentTab.SEND_FILES,
                            onClick = { currentTab = XentTab.SEND_FILES },
                            icon = { Icon(Icons.Default.UploadFile, contentDescription = "Send") },
                            label = { Text("Send", maxLines = 1) }
                        )
                        NavigationBarItem(
                            selected = currentTab == XentTab.TRANSFERS,
                            onClick = { currentTab = XentTab.TRANSFERS },
                            icon = {
                                BadgedBox(badge = {
                                    if (activeTransfersCount > 0) Badge { Text("$activeTransfersCount") }
                                }) {
                                    Icon(Icons.Default.SwapVert, contentDescription = "Transfers")
                                }
                            },
                            label = { Text("Transfers", maxLines = 1) }
                        )
                    }
                }
            }
        ) { innerPadding ->
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .background(MaterialTheme.colorScheme.background)
                    .padding(innerPadding)
            ) {
                val chatPeerId = activePeerId
                if (chatPeerId != null) {
                    val peer = peers.firstOrNull { it.deviceId == chatPeerId }
                    ChatThreadScreen(
                        peerId = chatPeerId,
                        peer = peer,
                        settings = settings,
                        messages = messages.filter { it.threadPeerId == chatPeerId },
                        transfers = transfers.filter { it.senderId == chatPeerId || it.receiverId == chatPeerId },
                        isTyping = (typingMap[chatPeerId] ?: 0L) > System.currentTimeMillis() - 3500L,
                        onBack = { activePeerId = null },
                        onSendMessage = { txt, quote -> engine.sendMessage(chatPeerId, txt, quote) },
                        onTyping = { engine.sendTyping(chatPeerId) },
                        onMarkRead = { engine.markThreadRead(chatPeerId) },
                        onRetryMessage = { engine.retryMessage(it) },
                        onEditMessage = { id, content -> engine.editMessage(chatPeerId, id, content) },
                        onDeleteMessage = { id -> engine.deleteMessage(chatPeerId, id) },
                        onReaction = { id, emoji -> engine.toggleReaction(chatPeerId, id, emoji) },
                        onPickFilesForPeer = {
                            if (peer?.online == true) {
                                chatAttachPicker.launch(arrayOf("*/*"))
                            } else {
                                Toast.makeText(context, "Device is offline; connect first to send files.", Toast.LENGTH_SHORT).show()
                            }
                        },
                        onPauseTransfer = { engine.pauseTransfer(it) },
                        onResumeTransfer = { engine.resumeTransfer(it) },
                        onCancelTransfer = { engine.cancelTransfer(it) },
                        onAcceptTransfer = { engine.respondToOffer(it, true) },
                        onDeclineTransfer = { engine.respondToOffer(it, false) },
                        onOpenFile = { path ->
                            val err = onOpenReceivedFile(path)
                            if (err != null) Toast.makeText(context, err, Toast.LENGTH_SHORT).show()
                        }
                    )
                } else {
                    when (currentTab) {
                        XentTab.MESSAGES -> MessagesInboxScreen(
                            peers = peers,
                            messages = messages,
                            settings = settings,
                            onOpenChat = { peerId ->
                                activePeerId = peerId
                                engine.markThreadRead(peerId)
                            },
                            onGoNearby = { currentTab = XentTab.NEARBY }
                        )

                        XentTab.NEARBY -> NearbyDevicesScreen(
                            peers = peers,
                            settings = settings,
                            onMessagePeer = { peerId ->
                                activePeerId = peerId
                                engine.markThreadRead(peerId)
                            },
                            onSendFilesToPeer = { peerId ->
                                preselectedRecipientId = peerId
                                currentTab = XentTab.SEND_FILES
                            },
                            onToggleFavorite = { peerId ->
                                engine.store.updateSettings { s ->
                                    val fav = if (s.favoritePeers.contains(peerId)) {
                                        s.favoritePeers - peerId
                                    } else {
                                        s.favoritePeers + peerId
                                    }
                                    s.copy(favoritePeers = fav)
                                }
                            },
                            onToggleBlocked = { peerId ->
                                engine.store.updateSettings { s ->
                                    val blk = if (s.blockedPeers.contains(peerId)) {
                                        s.blockedPeers - peerId
                                    } else {
                                        s.blockedPeers + peerId
                                    }
                                    s.copy(blockedPeers = blk, favoritePeers = s.favoritePeers - peerId)
                                }
                            },
                            onConnectIp = { showConnectIp = true }
                        )

                        XentTab.SEND_FILES -> SendFilesScreen(
                            peers = peers,
                            settings = settings,
                            queuedFiles = queuedFiles,
                            selectedPeerId = preselectedRecipientId,
                            onSelectPeer = { preselectedRecipientId = it },
                            onPickFiles = { multiFilePicker.launch(arrayOf("*/*")) },
                            onRemoveFile = { id -> queuedFiles = queuedFiles.filterNot { it.id == id } },
                            onSendAll = { targetPeerId ->
                                val filesSnapshot = queuedFiles
                                queuedFiles = emptyList()
                                currentTab = XentTab.TRANSFERS
                                scope.launch {
                                    for (f in filesSnapshot) {
                                        engine.sendFileRequest(targetPeerId, f)
                                    }
                                }
                            }
                        )

                        XentTab.TRANSFERS -> TransfersScreen(
                            transfers = transfers,
                            onPause = { engine.pauseTransfer(it) },
                            onResume = { engine.resumeTransfer(it) },
                            onCancel = { engine.cancelTransfer(it) },
                            onAccept = { engine.respondToOffer(it, true) },
                            onDecline = { engine.respondToOffer(it, false) },
                            onOpenFile = { path ->
                                val err = onOpenReceivedFile(path)
                                if (err != null) Toast.makeText(context, err, Toast.LENGTH_SHORT).show()
                            },
                            onClearCompleted = { engine.store.clearFinishedTransfers() }
                        )
                    }
                }
            }
        }

        if (incomingOffer != null) {
            val offer = incomingOffer!!
            AlertDialog(
                onDismissRequest = { },
                title = { Text("Incoming File Offer") },
                text = {
                    Column {
                        Text("${offer.senderName} wants to send you:")
                        Spacer(Modifier.height(8.dp))
                        Text(offer.fileName, fontWeight = FontWeight.SemiBold)
                        Text(
                            XentFormat.formatBytes(offer.fileSize),
                            color = MaterialTheme.colorScheme.onSurfaceVariant
                        )
                    }
                },
                confirmButton = {
                    Button(onClick = {
                        engine.respondToOffer(offer.transferId, accept = true)
                        currentTab = XentTab.TRANSFERS
                    }) {
                        Text("Accept")
                    }
                },
                dismissButton = {
                    TextButton(onClick = { engine.respondToOffer(offer.transferId, accept = false) }) {
                        Text("Decline")
                    }
                }
            )
        }

        if (showConnectIp) {
            ConnectByIpDialog(
                onDismiss = { showConnectIp = false },
                onConnect = { ipText ->
                    scope.launch {
                        val res = engine.addPeerByIp(ipText)
                        if (res.isSuccess) {
                            Toast.makeText(context, "Connected to ${res.getOrNull()?.alias}", Toast.LENGTH_SHORT).show()
                            showConnectIp = false
                        } else {
                            Toast.makeText(context, "Failed: ${res.exceptionOrNull()?.message}", Toast.LENGTH_LONG).show()
                        }
                    }
                }
            )
        }

        if (showSettings) {
            SettingsDialog(
                settings = settings,
                boundPort = engine.boundPort,
                downloadPath = engine.downloadDir.absolutePath,
                fingerprint = engine.tls.fingerprintHex,
                onDismiss = { showSettings = false },
                onSaveAlias = { alias -> engine.store.updateSettings { it.copy(alias = alias.trim()) } },
                onSetTheme = { theme -> engine.store.updateSettings { it.copy(theme = theme) } },
                onSetStatus = { st -> engine.store.updateSettings { it.copy(status = st) } },
                onToggleAutoAccept = { v -> engine.store.updateSettings { it.copy(autoAcceptFavorites = v) } },
                onToggleDiscovery = { v -> engine.store.updateSettings { it.copy(discoveryEnabled = v) } },
                onToggleSound = { v -> engine.store.updateSettings { it.copy(soundEnabled = v) } }
            )
        }
    }
}

@Composable
private fun displayPeerName(peerId: String, fallback: String, settings: XentSettings): String =
    settings.peerNicknames[peerId]?.takeIf { it.isNotBlank() } ?: fallback

@Composable
private fun MessagesInboxScreen(
    peers: List<PeerInfo>,
    messages: List<ChatMessage>,
    settings: XentSettings,
    onOpenChat: (String) -> Unit,
    onGoNearby: () -> Unit
) {
    val threadIds = (messages.map { it.threadPeerId } + peers.filter { it.online }.map { it.deviceId }).distinct()
    if (threadIds.isEmpty()) {
        Box(modifier = Modifier.fillMaxSize().padding(32.dp), contentAlignment = Alignment.Center) {
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Icon(
                    Icons.Default.ChatBubbleOutline,
                    contentDescription = null,
                    modifier = Modifier.size(48.dp),
                    tint = MaterialTheme.colorScheme.onSurfaceVariant
                )
                Spacer(Modifier.height(12.dp))
                Text("No conversations yet", style = MaterialTheme.typography.titleMedium)
                Spacer(Modifier.height(4.dp))
                Text(
                    "Discover a nearby Xent device to start messaging and sharing files.",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyMedium
                )
                Spacer(Modifier.height(16.dp))
                Button(onClick = onGoNearby) { Text("Find Nearby Devices") }
            }
        }
        return
    }

    val sortedThreads = threadIds.sortedByDescending { id ->
        messages.lastOrNull { it.threadPeerId == id }?.createdAt
            ?: peers.firstOrNull { it.deviceId == id }?.lastSeenMs
            ?: 0L
    }

    LazyColumn(modifier = Modifier.fillMaxSize()) {
        items(sortedThreads, key = { it }) { peerId ->
            val peer = peers.firstOrNull { it.deviceId == peerId }
            val lastMsg = messages.lastOrNull { it.threadPeerId == peerId }
            val unread = messages.count { it.threadPeerId == peerId && it.isIncoming && !it.isRead }
            val rawName = peer?.alias ?: lastMsg?.let { if (it.isIncoming) it.senderName else it.receiverName } ?: "Device"
            val name = displayPeerName(peerId, rawName, settings)

            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .clickable { onOpenChat(peerId) }
                    .padding(horizontal = 16.dp, vertical = 14.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Box(
                    modifier = Modifier
                        .size(44.dp)
                        .clip(CircleShape)
                        .background(MaterialTheme.colorScheme.surfaceVariant),
                    contentAlignment = Alignment.Center
                ) {
                    Text(
                        name.firstOrNull()?.uppercase() ?: "X",
                        fontWeight = FontWeight.Bold,
                        color = MaterialTheme.colorScheme.primary
                    )
                }
                Spacer(Modifier.width(12.dp))
                Column(modifier = Modifier.weight(1f)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(
                            text = name,
                            fontWeight = FontWeight.SemiBold,
                            modifier = Modifier.weight(1f),
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis
                        )
                        if (lastMsg != null) {
                            Text(
                                text = XentFormat.formatTime(lastMsg.createdAt),
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant
                            )
                        }
                    }
                    Spacer(Modifier.height(2.dp))
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Box(
                            modifier = Modifier
                                .size(8.dp)
                                .clip(CircleShape)
                                .background(
                                    if (peer?.online == true) XentColors.OnlineGreen
                                    else MaterialTheme.colorScheme.outline
                                )
                        )
                        Spacer(Modifier.width(6.dp))
                        Text(
                            text = lastMsg?.content ?: if (peer?.online == true) "Online • Tap to chat" else "Offline",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f)
                        )
                        if (unread > 0) {
                            Spacer(Modifier.width(8.dp))
                            Badge { Text("$unread") }
                        }
                    }
                }
            }
            HorizontalDivider(color = MaterialTheme.colorScheme.outline.copy(alpha = 0.4f))
        }
    }
}

private sealed class ChatTimelineEntry(val key: String, val createdAt: Long) {
    class Msg(val msg: ChatMessage) : ChatTimelineEntry("m_${msg.id}", msg.createdAt)
    class Tx(val tx: TransferItem) : ChatTimelineEntry("t_${tx.transferId}", tx.createdAt)
}

@OptIn(ExperimentalMaterial3Api::class, ExperimentalFoundationApi::class)
@Composable
private fun ChatThreadScreen(
    peerId: String,
    peer: PeerInfo?,
    settings: XentSettings,
    messages: List<ChatMessage>,
    transfers: List<TransferItem>,
    isTyping: Boolean,
    onBack: () -> Unit,
    onSendMessage: (String, String?) -> Unit,
    onTyping: () -> Unit,
    onMarkRead: () -> Unit,
    onRetryMessage: (ChatMessage) -> Unit,
    onEditMessage: (String, String) -> Unit,
    onDeleteMessage: (String) -> Unit,
    onReaction: (String, String) -> Unit,
    onPickFilesForPeer: () -> Unit,
    onPauseTransfer: (String) -> Unit,
    onResumeTransfer: (String) -> Unit,
    onCancelTransfer: (String) -> Unit,
    onAcceptTransfer: (String) -> Unit,
    onDeclineTransfer: (String) -> Unit,
    onOpenFile: (String) -> Unit
) {
    val context = LocalContext.current
    var draft by remember { mutableStateOf("") }
    var quoteText by remember { mutableStateOf<String?>(null) }
    var searchOpen by remember { mutableStateOf(false) }
    var searchQuery by remember { mutableStateOf("") }
    var menuMessage by remember { mutableStateOf<ChatMessage?>(null) }
    var editingMessage by remember { mutableStateOf<ChatMessage?>(null) }
    var editDraft by remember { mutableStateOf("") }

    val listState = rememberLazyListState()
    val timeline = remember(messages, transfers, searchQuery) {
        val q = searchQuery.trim()
        val msgEntries = messages
            .filter { q.isBlank() || it.content.contains(q, ignoreCase = true) }
            .map { ChatTimelineEntry.Msg(it) }
        val txEntries = transfers
            .filter { q.isBlank() || it.fileName.contains(q, ignoreCase = true) }
            .map { ChatTimelineEntry.Tx(it) }
        (msgEntries + txEntries).sortedBy { it.createdAt }
    }

    LaunchedEffect(timeline.size) {
        onMarkRead()
        if (timeline.isNotEmpty()) {
            listState.animateScrollToItem(timeline.size - 1)
        }
    }

    val fallbackName = peer?.alias ?: messages.lastOrNull()?.let { if (it.isIncoming) it.senderName else it.receiverName } ?: "Device"
    val displayName = displayPeerName(peerId, fallbackName, settings)

    Column(modifier = Modifier.fillMaxSize()) {
        TopAppBar(
            navigationIcon = {
                IconButton(onClick = onBack) {
                    Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                }
            },
            title = {
                Column {
                    Text(displayName, fontWeight = FontWeight.SemiBold, fontSize = 17.sp)
                    Text(
                        text = when {
                            isTyping -> "typing..."
                            peer?.online == true -> "Online • ${peer.ip}:${peer.port}"
                            else -> "Offline • queued messages deliver when back"
                        },
                        style = MaterialTheme.typography.labelSmall,
                        color = if (isTyping || peer?.online == true) XentColors.OnlineGreen else MaterialTheme.colorScheme.onSurfaceVariant
                    )
                }
            },
            actions = {
                IconButton(onClick = { searchOpen = !searchOpen; if (!searchOpen) searchQuery = "" }) {
                    Icon(Icons.Default.Search, contentDescription = "Search messages")
                }
                IconButton(onClick = onPickFilesForPeer) {
                    Icon(Icons.Default.AttachFile, contentDescription = "Send files")
                }
            },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.surface)
        )

        if (searchOpen) {
            OutlinedTextField(
                value = searchQuery,
                onValueChange = { searchQuery = it },
                placeholder = { Text("Search in conversation...") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 6.dp),
                trailingIcon = {
                    if (searchQuery.isNotEmpty()) {
                        IconButton(onClick = { searchQuery = "" }) {
                            Icon(Icons.Default.Close, contentDescription = "Clear")
                        }
                    }
                }
            )
        }

        LazyColumn(
            state = listState,
            modifier = Modifier.weight(1f).fillMaxWidth(),
            contentPadding = PaddingValues(12.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            items(timeline, key = { it.key }) { entry ->
                if (entry is ChatTimelineEntry.Tx) {
                    val t = entry.tx
                    val align = if (t.isIncoming) Alignment.Start else Alignment.End
                    Column(modifier = Modifier.fillMaxWidth(), horizontalAlignment = align) {
                        Card(
                            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                            modifier = Modifier
                                .widthIn(max = 300.dp)
                                .border(1.dp, MaterialTheme.colorScheme.outline, RoundedCornerShape(12.dp))
                        ) {
                            Column(modifier = Modifier.padding(12.dp)) {
                                Text(t.fileName, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                Text(
                                    text = buildString {
                                        append(if (t.isIncoming) "Incoming" else "Outgoing")
                                        append(" • ${XentFormat.formatBytes(t.transferredBytes)} / ${XentFormat.formatBytes(t.fileSize)}")
                                        if (t.speedMbps > 0.05 && t.status == TransferStatus.TRANSFERRING) {
                                            append(" • %.1f MB/s".format(t.speedMbps))
                                        }
                                    },
                                    style = MaterialTheme.typography.labelSmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant
                                )
                                if (t.status == TransferStatus.TRANSFERRING || t.status == TransferStatus.PAUSED) {
                                    Spacer(Modifier.height(6.dp))
                                    LinearProgressIndicator(progress = { t.progressFraction }, modifier = Modifier.fillMaxWidth())
                                }
                                Spacer(Modifier.height(6.dp))
                                Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                                    Text(
                                        text = t.status.name,
                                        fontSize = 11.sp,
                                        color = if (t.status == TransferStatus.COMPLETED) XentColors.OnlineGreen else MaterialTheme.colorScheme.primary
                                    )
                                    Spacer(Modifier.weight(1f))
                                    if (t.isIncoming && t.status == TransferStatus.PENDING) {
                                        TextButton(onClick = { onAcceptTransfer(t.transferId) }) { Text("Accept") }
                                        TextButton(onClick = { onDeclineTransfer(t.transferId) }) { Text("Decline") }
                                    } else if (t.status == TransferStatus.TRANSFERRING) {
                                        TextButton(onClick = { onPauseTransfer(t.transferId) }) { Text("Pause") }
                                        TextButton(onClick = { onCancelTransfer(t.transferId) }) { Text("Cancel") }
                                    } else if (t.status == TransferStatus.PAUSED) {
                                        TextButton(onClick = { onResumeTransfer(t.transferId) }) { Text("Resume") }
                                        TextButton(onClick = { onCancelTransfer(t.transferId) }) { Text("Cancel") }
                                    } else if (t.isIncoming && t.status == TransferStatus.COMPLETED && !t.filePath.isNullOrBlank()) {
                                        TextButton(onClick = { onOpenFile(t.filePath) }) { Text("Open") }
                                    }
                                }
                            }
                        }
                    }
                    return@items
                }
                val msg = (entry as ChatTimelineEntry.Msg).msg
                val align = if (msg.isIncoming) Alignment.Start else Alignment.End
                val bg = if (msg.isIncoming) MaterialTheme.colorScheme.surfaceVariant else MaterialTheme.colorScheme.primary
                val fg = if (msg.isIncoming) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onPrimary

                Column(modifier = Modifier.fillMaxWidth(), horizontalAlignment = align) {
                    Box {
                        Column(
                            modifier = Modifier
                                .widthIn(max = 300.dp)
                                .clip(RoundedCornerShape(14.dp))
                                .background(bg)
                                .combinedClickable(
                                    onClick = { },
                                    onLongClick = { menuMessage = msg }
                                )
                                .padding(horizontal = 12.dp, vertical = 8.dp)
                        ) {
                            if (!msg.quote.isNullOrBlank()) {
                                Text(
                                    text = msg.quote,
                                    fontSize = 11.sp,
                                    color = fg.copy(alpha = 0.8f),
                                    modifier = Modifier
                                        .padding(bottom = 4.dp)
                                        .background(Color.Black.copy(alpha = 0.12f), RoundedCornerShape(6.dp))
                                        .padding(6.dp),
                                    maxLines = 2,
                                    overflow = TextOverflow.Ellipsis
                                )
                            }
                            Text(
                                text = msg.content,
                                color = if (msg.isDeleted) fg.copy(alpha = 0.65f) else fg,
                                fontStyle = if (msg.isDeleted) FontStyle.Italic else FontStyle.Normal,
                                fontSize = 15.sp
                            )
                            Spacer(Modifier.height(4.dp))
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Text(
                                    text = buildString {
                                        append(XentFormat.formatTime(msg.createdAt))
                                        if (msg.isEdited && !msg.isDeleted) append(" (edited)")
                                        if (!msg.isIncoming) {
                                            append(" • ")
                                            append(
                                                when {
                                                    msg.status.startsWith("Read") -> "Read"
                                                    msg.status.startsWith("Delivered") -> "Delivered"
                                                    else -> msg.status
                                                }
                                            )
                                        }
                                    },
                                    fontSize = 10.sp,
                                    color = fg.copy(alpha = 0.72f)
                                )
                                if (!msg.isIncoming && msg.status.startsWith("Read")) {
                                    Spacer(Modifier.width(4.dp))
                                    Icon(Icons.Default.DoneAll, contentDescription = "Read", modifier = Modifier.size(12.dp), tint = fg)
                                } else if (!msg.isIncoming && msg.status.startsWith("Delivered")) {
                                    Spacer(Modifier.width(4.dp))
                                    Icon(Icons.Default.Check, contentDescription = "Delivered", modifier = Modifier.size(12.dp), tint = fg.copy(alpha = 0.8f))
                                }
                            }
                        }

                        DropdownMenu(
                            expanded = menuMessage?.id == msg.id,
                            onDismissRequest = { menuMessage = null }
                        ) {
                            if (!msg.isDeleted) {
                                DropdownMenuItem(
                                    text = { Text("Reply") },
                                    leadingIcon = { Icon(Icons.AutoMirrored.Filled.Reply, null) },
                                    onClick = {
                                        quoteText = "${msg.senderName}: ${msg.content.take(80)}"
                                        menuMessage = null
                                    }
                                )
                                DropdownMenuItem(
                                    text = { Text("Copy") },
                                    leadingIcon = { Icon(Icons.Default.ContentCopy, null) },
                                    onClick = {
                                        val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                                        cm.setPrimaryClip(ClipData.newPlainText("Message", msg.content))
                                        menuMessage = null
                                    }
                                )
                                DropdownMenuItem(
                                    text = { Text("React 👍  ❤️  🔥") },
                                    onClick = {
                                        onReaction(msg.id, "👍")
                                        menuMessage = null
                                    }
                                )
                                if (!msg.isIncoming) {
                                    DropdownMenuItem(
                                        text = { Text("Edit") },
                                        leadingIcon = { Icon(Icons.Default.Edit, null) },
                                        onClick = {
                                            editingMessage = msg
                                            editDraft = msg.content
                                            menuMessage = null
                                        }
                                    )
                                    DropdownMenuItem(
                                        text = { Text("Delete") },
                                        leadingIcon = { Icon(Icons.Default.Delete, null) },
                                        onClick = {
                                            onDeleteMessage(msg.id)
                                            menuMessage = null
                                        }
                                    )
                                    if (msg.status == "Failed" || msg.status == "Queued") {
                                        DropdownMenuItem(
                                            text = { Text("Retry") },
                                            leadingIcon = { Icon(Icons.Default.Refresh, null) },
                                            onClick = {
                                                onRetryMessage(msg)
                                                menuMessage = null
                                            }
                                        )
                                    }
                                }
                            }
                        }
                    }

                    if (msg.reactions.isNotEmpty()) {
                        Row(horizontalArrangement = Arrangement.spacedBy(4.dp), modifier = Modifier.padding(top = 3.dp)) {
                            msg.reactions.forEach { (emoji, users) ->
                                Row(
                                    modifier = Modifier
                                        .clip(RoundedCornerShape(12.dp))
                                        .border(1.dp, MaterialTheme.colorScheme.outline, RoundedCornerShape(12.dp))
                                        .background(MaterialTheme.colorScheme.surface)
                                        .clickable { onReaction(msg.id, emoji) }
                                        .padding(horizontal = 7.dp, vertical = 2.dp),
                                    verticalAlignment = Alignment.CenterVertically
                                ) {
                                    Text("$emoji ${users.size}", fontSize = 11.sp)
                                }
                            }
                        }
                    }
                }
            }
        }

        if (quoteText != null) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .background(MaterialTheme.colorScheme.surfaceVariant)
                    .padding(horizontal = 12.dp, vertical = 6.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text(
                    text = quoteText!!,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                    style = MaterialTheme.typography.bodySmall
                )
                IconButton(onClick = { quoteText = null }, modifier = Modifier.size(24.dp)) {
                    Icon(Icons.Default.Close, contentDescription = "Cancel reply", modifier = Modifier.size(16.dp))
                }
            }
        }

        HorizontalDivider(color = MaterialTheme.colorScheme.outline.copy(alpha = 0.5f))
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .background(MaterialTheme.colorScheme.surface)
                .padding(horizontal = 8.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            IconButton(onClick = onPickFilesForPeer) {
                Icon(Icons.Default.AttachFile, contentDescription = "Attach files")
            }
            OutlinedTextField(
                value = draft,
                onValueChange = {
                    draft = it
                    onTyping()
                },
                placeholder = { Text("Message $displayName") },
                modifier = Modifier.weight(1f),
                maxLines = 4,
                shape = RoundedCornerShape(22.dp)
            )
            Spacer(Modifier.width(6.dp))
            IconButton(
                onClick = {
                    if (draft.isNotBlank()) {
                        onSendMessage(draft, quoteText)
                        draft = ""
                        quoteText = null
                    }
                },
                enabled = draft.isNotBlank()
            ) {
                Icon(
                    Icons.AutoMirrored.Filled.Send,
                    contentDescription = "Send",
                    tint = if (draft.isNotBlank()) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant
                )
            }
        }
    }

    if (editingMessage != null) {
        val target = editingMessage!!
        AlertDialog(
            onDismissRequest = { editingMessage = null },
            title = { Text("Edit Message") },
            text = {
                OutlinedTextField(
                    value = editDraft,
                    onValueChange = { editDraft = it },
                    modifier = Modifier.fillMaxWidth()
                )
            },
            confirmButton = {
                Button(onClick = {
                    if (editDraft.isNotBlank()) {
                        onEditMessage(target.id, editDraft)
                    }
                    editingMessage = null
                }) { Text("Save") }
            },
            dismissButton = {
                TextButton(onClick = { editingMessage = null }) { Text("Cancel") }
            }
        )
    }
}

@Composable
private fun NearbyDevicesScreen(
    peers: List<PeerInfo>,
    settings: XentSettings,
    onMessagePeer: (String) -> Unit,
    onSendFilesToPeer: (String) -> Unit,
    onToggleFavorite: (String) -> Unit,
    onToggleBlocked: (String) -> Unit,
    onConnectIp: () -> Unit
) {
    var search by remember { mutableStateOf("") }
    var filter by remember { mutableStateOf("all") }

    val filtered = remember(peers, search, filter, settings) {
        peers.filter { p ->
            val q = search.trim()
            val matchesQ = q.isEmpty() || p.alias.contains(q, true) || p.ip.contains(q, true) ||
                (settings.peerNicknames[p.deviceId]?.contains(q, true) == true)
            val isFav = settings.favoritePeers.contains(p.deviceId)
            val matchesType = when (filter) {
                "online" -> p.online
                "favorites" -> isFav
                "desktop" -> !p.deviceType.equals("mobile", ignoreCase = true)
                "mobile" -> p.deviceType.equals("mobile", ignoreCase = true)
                else -> true
            }
            matchesQ && matchesType
        }
    }

    Column(modifier = Modifier.fillMaxSize()) {
        OutlinedTextField(
            value = search,
            onValueChange = { search = it },
            placeholder = { Text("Search devices or IP...") },
            leadingIcon = { Icon(Icons.Default.Search, null) },
            singleLine = true,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp)
        )
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            listOf("all" to "All", "online" to "Online", "favorites" to "Favorites", "desktop" to "Desktop", "mobile" to "Mobile")
                .forEach { (key, label) ->
                    FilterChip(
                        selected = filter == key,
                        onClick = { filter = key },
                        label = { Text(label, fontSize = 12.sp) }
                    )
                }
        }

        if (filtered.isEmpty()) {
            Box(modifier = Modifier.fillMaxSize().padding(32.dp), contentAlignment = Alignment.Center) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Text("No devices found", style = MaterialTheme.typography.titleMedium)
                    Spacer(Modifier.height(6.dp))
                    Text(
                        "Ensure both devices are on the same Wi-Fi/LAN or connect manually by IP.",
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        style = MaterialTheme.typography.bodySmall
                    )
                    Spacer(Modifier.height(14.dp))
                    OutlinedButton(onClick = onConnectIp) { Text("Connect by IP") }
                }
            }
        } else {
            LazyColumn(contentPadding = PaddingValues(12.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                items(filtered, key = { it.deviceId }) { peer ->
                    val isFav = settings.favoritePeers.contains(peer.deviceId)
                    val isBlocked = settings.blockedPeers.contains(peer.deviceId)
                    val isMobile = peer.deviceType.equals("mobile", ignoreCase = true)
                    val name = displayPeerName(peer.deviceId, peer.alias, settings)

                    Card(
                        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                        modifier = Modifier
                            .fillMaxWidth()
                            .border(1.dp, MaterialTheme.colorScheme.outline, RoundedCornerShape(12.dp))
                    ) {
                        Column(modifier = Modifier.padding(14.dp)) {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Icon(
                                    imageVector = if (isMobile) Icons.Default.PhoneAndroid else Icons.Default.Computer,
                                    contentDescription = null,
                                    tint = MaterialTheme.colorScheme.primary
                                )
                                Spacer(Modifier.width(10.dp))
                                Column(modifier = Modifier.weight(1f)) {
                                    Text(name, fontWeight = FontWeight.SemiBold)
                                    Text(
                                        text = buildString {
                                            append(if (peer.online) "Online" else "Offline")
                                            if (peer.ip.isNotBlank()) append(" • ${peer.ip}:${peer.port}")
                                            peer.os?.let { append(" • $it") }
                                        },
                                        style = MaterialTheme.typography.labelSmall,
                                        color = if (peer.online) XentColors.OnlineGreen else MaterialTheme.colorScheme.onSurfaceVariant
                                    )
                                }
                                IconButton(onClick = { onToggleFavorite(peer.deviceId) }) {
                                    Icon(
                                        if (isFav) Icons.Default.Star else Icons.Default.StarBorder,
                                        contentDescription = "Favorite",
                                        tint = if (isFav) XentColors.AwayAmber else MaterialTheme.colorScheme.onSurfaceVariant
                                    )
                                }
                                IconButton(onClick = { onToggleBlocked(peer.deviceId) }) {
                                    Icon(
                                        Icons.Default.Block,
                                        contentDescription = "Block",
                                        tint = if (isBlocked) XentColors.BusyRed else MaterialTheme.colorScheme.onSurfaceVariant
                                    )
                                }
                            }
                            Spacer(Modifier.height(10.dp))
                            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                OutlinedButton(
                                    onClick = { onMessagePeer(peer.deviceId) },
                                    modifier = Modifier.weight(1f)
                                ) {
                                    Text("Message")
                                }
                                Button(
                                    onClick = { onSendFilesToPeer(peer.deviceId) },
                                    enabled = peer.online && !isBlocked,
                                    modifier = Modifier.weight(1f)
                                ) {
                                    Text("Send Files")
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun SendFilesScreen(
    peers: List<PeerInfo>,
    settings: XentSettings,
    queuedFiles: List<QueuedUploadSource>,
    selectedPeerId: String?,
    onSelectPeer: (String) -> Unit,
    onPickFiles: () -> Unit,
    onRemoveFile: (String) -> Unit,
    onSendAll: (String) -> Unit
) {
    val onlinePeers = peers.filter { it.online && !settings.blockedPeers.contains(it.deviceId) }
    val effectiveRecipient = selectedPeerId?.takeIf { id -> onlinePeers.any { it.deviceId == id } }
        ?: onlinePeers.firstOrNull()?.deviceId

    Column(modifier = Modifier.fillMaxSize().padding(16.dp)) {
        OutlinedButton(
            onClick = onPickFiles,
            modifier = Modifier.fillMaxWidth().height(56.dp),
            shape = RoundedCornerShape(12.dp)
        ) {
            Icon(Icons.Default.AttachFile, null)
            Spacer(Modifier.width(8.dp))
            Text("Select Files to Send")
        }

        Spacer(Modifier.height(16.dp))
        Text("Recipient Device", fontWeight = FontWeight.SemiBold, style = MaterialTheme.typography.labelLarge)
        Spacer(Modifier.height(6.dp))
        if (onlinePeers.isEmpty()) {
            Text(
                "No online peers available right now. Open Nearby to discover or add by IP.",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodySmall
            )
        } else {
            LazyColumn(modifier = Modifier.height(120.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                items(onlinePeers, key = { it.deviceId }) { peer ->
                    val selected = peer.deviceId == effectiveRecipient
                    val name = displayPeerName(peer.deviceId, peer.alias, settings)
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clip(RoundedCornerShape(10.dp))
                            .border(
                                width = if (selected) 2.dp else 1.dp,
                                color = if (selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outline,
                                shape = RoundedCornerShape(10.dp)
                            )
                            .clickable { onSelectPeer(peer.deviceId) }
                            .padding(horizontal = 12.dp, vertical = 10.dp),
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Text(name, fontWeight = FontWeight.Medium, modifier = Modifier.weight(1f))
                        Text(peer.ip, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
            }
        }

        Spacer(Modifier.height(16.dp))
        Text("Selected Files (${queuedFiles.size})", fontWeight = FontWeight.SemiBold, style = MaterialTheme.typography.labelLarge)
        Spacer(Modifier.height(6.dp))

        LazyColumn(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            items(queuedFiles, key = { it.id }) { file ->
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(10.dp))
                        .background(MaterialTheme.colorScheme.surface)
                        .border(1.dp, MaterialTheme.colorScheme.outline, RoundedCornerShape(10.dp))
                        .padding(horizontal = 12.dp, vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Column(modifier = Modifier.weight(1f)) {
                        Text(file.name, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text(XentFormat.formatBytes(file.size), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    IconButton(onClick = { onRemoveFile(file.id) }) {
                        Icon(Icons.Default.Close, contentDescription = "Remove")
                    }
                }
            }
        }

        Spacer(Modifier.height(12.dp))
        Button(
            onClick = { effectiveRecipient?.let(onSendAll) },
            enabled = effectiveRecipient != null && queuedFiles.isNotEmpty(),
            modifier = Modifier.fillMaxWidth().height(48.dp),
            shape = RoundedCornerShape(10.dp)
        ) {
            Text("Send ${queuedFiles.size} File${if (queuedFiles.size == 1) "" else "s"}")
        }
    }
}

@Composable
private fun TransfersScreen(
    transfers: List<TransferItem>,
    onPause: (String) -> Unit,
    onResume: (String) -> Unit,
    onCancel: (String) -> Unit,
    onAccept: (String) -> Unit,
    onDecline: (String) -> Unit,
    onOpenFile: (String) -> Unit,
    onClearCompleted: () -> Unit
) {
    var filter by remember { mutableStateOf("all") }
    val filtered = remember(transfers, filter) {
        transfers.filter { t ->
            when (filter) {
                "incoming" -> t.isIncoming
                "outgoing" -> !t.isIncoming
                "active" -> t.status.isActive()
                "completed" -> t.status == TransferStatus.COMPLETED
                else -> true
            }
        }
    }

    Column(modifier = Modifier.fillMaxSize()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            listOf("all" to "All", "incoming" to "In", "outgoing" to "Out", "active" to "Active", "completed" to "Done")
                .forEach { (key, label) ->
                    FilterChip(
                        selected = filter == key,
                        onClick = { filter = key },
                        label = { Text(label, fontSize = 12.sp) }
                    )
                }
            Spacer(Modifier.weight(1f))
            if (transfers.any { it.status.isTerminal() }) {
                TextButton(onClick = onClearCompleted) { Text("Clear") }
            }
        }

        if (filtered.isEmpty()) {
            Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text("No transfers match filter", color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        } else {
            LazyColumn(contentPadding = PaddingValues(12.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                items(filtered, key = { it.transferId }) { t ->
                    Card(
                        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                        modifier = Modifier
                            .fillMaxWidth()
                            .border(1.dp, MaterialTheme.colorScheme.outline, RoundedCornerShape(12.dp))
                    ) {
                        Column(modifier = Modifier.padding(14.dp)) {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Column(modifier = Modifier.weight(1f)) {
                                    Text(t.fileName, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                    Text(
                                        text = buildString {
                                            append(if (t.isIncoming) "From ${t.senderName}" else "To ${t.receiverName}")
                                            append(" • ${XentFormat.formatBytes(t.transferredBytes)} / ${XentFormat.formatBytes(t.fileSize)}")
                                            if (t.speedMbps > 0.05 && t.status == TransferStatus.TRANSFERRING) {
                                                append(" • %.1f MB/s".format(t.speedMbps))
                                            }
                                        },
                                        style = MaterialTheme.typography.labelSmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant
                                    )
                                }
                                Text(
                                    text = t.status.name,
                                    fontSize = 11.sp,
                                    fontWeight = FontWeight.Medium,
                                    color = when (t.status) {
                                        TransferStatus.COMPLETED -> XentColors.OnlineGreen
                                        TransferStatus.FAILED, TransferStatus.DECLINED -> XentColors.BusyRed
                                        TransferStatus.PAUSED -> XentColors.AwayAmber
                                        else -> MaterialTheme.colorScheme.primary
                                    }
                                )
                            }

                            if (t.status == TransferStatus.TRANSFERRING || t.status == TransferStatus.PAUSED) {
                                Spacer(Modifier.height(8.dp))
                                LinearProgressIndicator(
                                    progress = { t.progressFraction },
                                    modifier = Modifier.fillMaxWidth()
                                )
                            }

                            Spacer(Modifier.height(8.dp))
                            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                if (t.isIncoming && t.status == TransferStatus.PENDING) {
                                    Button(onClick = { onAccept(t.transferId) }) { Text("Accept") }
                                    OutlinedButton(onClick = { onDecline(t.transferId) }) { Text("Decline") }
                                }
                                if (t.status == TransferStatus.TRANSFERRING) {
                                    IconButton(onClick = { onPause(t.transferId) }) {
                                        Icon(Icons.Default.Pause, contentDescription = "Pause")
                                    }
                                }
                                if (t.status == TransferStatus.PAUSED) {
                                    IconButton(onClick = { onResume(t.transferId) }) {
                                        Icon(Icons.Default.PlayArrow, contentDescription = "Resume")
                                    }
                                }
                                if (t.status.isActive()) {
                                    IconButton(onClick = { onCancel(t.transferId) }) {
                                        Icon(Icons.Default.Cancel, contentDescription = "Cancel")
                                    }
                                }
                                if (t.isIncoming && t.status == TransferStatus.COMPLETED && !t.filePath.isNullOrBlank()) {
                                    OutlinedButton(onClick = { onOpenFile(t.filePath) }) {
                                        Icon(Icons.Default.FolderOpen, null, modifier = Modifier.size(16.dp))
                                        Spacer(Modifier.width(6.dp))
                                        Text("Open")
                                    }
                                }
                            }
                            if (!t.error.isNullOrBlank()) {
                                Text(t.error, color = XentColors.BusyRed, style = MaterialTheme.typography.labelSmall)
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun ConnectByIpDialog(
    onDismiss: () -> Unit,
    onConnect: (String) -> Unit
) {
    var ipInput by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Connect by IP") },
        text = {
            Column {
                Text("Enter peer IPv4 address (e.g. 192.168.1.42 or 192.168.1.42:53317):")
                Spacer(Modifier.height(8.dp))
                OutlinedTextField(
                    value = ipInput,
                    onValueChange = { ipInput = it },
                    placeholder = { Text("192.168.1.42") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth()
                )
            }
        },
        confirmButton = {
            Button(onClick = { if (ipInput.isNotBlank()) onConnect(ipInput) }) {
                Text("Connect")
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("Cancel") }
        }
    )
}

@Composable
private fun SettingsDialog(
    settings: XentSettings,
    boundPort: Int,
    downloadPath: String,
    fingerprint: String,
    onDismiss: () -> Unit,
    onSaveAlias: (String) -> Unit,
    onSetTheme: (String) -> Unit,
    onSetStatus: (String) -> Unit,
    onToggleAutoAccept: (Boolean) -> Unit,
    onToggleDiscovery: (Boolean) -> Unit,
    onToggleSound: (Boolean) -> Unit
) {
    var aliasDraft by remember { mutableStateOf(settings.alias) }
    var updateInfo by remember { mutableStateOf<AndroidUpdateInfo?>(null) }
    var updateStatus by remember { mutableStateOf("") }
    var checkingUpdates by remember { mutableStateOf(false) }
    var downloadingUpdate by remember { mutableStateOf(false) }
    var downloadProgress by remember { mutableStateOf(0f) }
    var downloadedApk by remember { mutableStateOf<java.io.File?>(null) }
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Xent Settings") },
        text = {
            LazyColumn(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                item {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Image(
                            painter = painterResource(id = R.drawable.ic_xent_logo),
                            contentDescription = "Xent",
                            modifier = Modifier.size(40.dp).clip(RoundedCornerShape(8.dp))
                        )
                        Spacer(Modifier.width(10.dp))
                        Column {
                            Text("Xent v${BuildConfig.VERSION_NAME}", fontWeight = FontWeight.Bold)
                            Text("LAN messenger & file sharing", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                }
                item {
                    Card(
                        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.45f)),
                        modifier = Modifier.fillMaxWidth().border(1.dp, MaterialTheme.colorScheme.outline, RoundedCornerShape(10.dp))
                    ) {
                        Column(modifier = Modifier.padding(10.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            Text("Software Update", fontWeight = FontWeight.SemiBold, style = MaterialTheme.typography.labelLarge)
                            if (updateStatus.isNotBlank()) {
                                Text(updateStatus, style = MaterialTheme.typography.bodySmall)
                            }
                            if (downloadingUpdate) {
                                LinearProgressIndicator(progress = { downloadProgress }, modifier = Modifier.fillMaxWidth())
                            }
                            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                OutlinedButton(
                                    enabled = !checkingUpdates && !downloadingUpdate,
                                    onClick = {
                                        checkingUpdates = true
                                        updateStatus = "Checking GitHub releases..."
                                        scope.launch {
                                            val res = XentUpdater.checkForUpdates(BuildConfig.VERSION_NAME)
                                            checkingUpdates = false
                                            if (res.isSuccess) {
                                                val info = res.getOrThrow()
                                                updateInfo = info
                                                updateStatus = if (info.hasUpdate) {
                                                    "New version v${info.latestVersion} available!"
                                                } else {
                                                    "You are on the latest version (v${info.currentVersion})."
                                                }
                                            } else {
                                                updateStatus = "Update check failed: ${res.exceptionOrNull()?.message}"
                                            }
                                        }
                                    }
                                ) { Text(if (checkingUpdates) "Checking..." else "Check Updates") }

                                val readyApk = downloadedApk
                                val dlUrl = updateInfo?.downloadUrl
                                if (readyApk != null) {
                                    Button(onClick = {
                                        val msg = XentUpdater.launchApkInstaller(ctx, readyApk)
                                        if (msg != null) updateStatus = msg
                                    }) { Text("Install") }
                                } else if (updateInfo?.hasUpdate == true && !dlUrl.isNullOrBlank()) {
                                    Button(
                                        enabled = !downloadingUpdate,
                                        onClick = {
                                            downloadingUpdate = true
                                            downloadProgress = 0f
                                            updateStatus = "Downloading ${updateInfo?.apkName ?: "update.apk"}..."
                                            scope.launch {
                                                val dlRes = XentUpdater.downloadApk(ctx, dlUrl) { p -> downloadProgress = p }
                                                downloadingUpdate = false
                                                if (dlRes.isSuccess) {
                                                    val f = dlRes.getOrThrow()
                                                    downloadedApk = f
                                                    val msg = XentUpdater.launchApkInstaller(ctx, f)
                                                    updateStatus = msg ?: "Installer launched for v${updateInfo?.latestVersion}."
                                                } else {
                                                    updateStatus = "Download failed: ${dlRes.exceptionOrNull()?.message}"
                                                }
                                            }
                                        }
                                    ) { Text("Download & Update") }
                                }
                            }
                        }
                    }
                }
                item {
                    OutlinedTextField(
                        value = aliasDraft,
                        onValueChange = { aliasDraft = it },
                        label = { Text("Device Name") },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth()
                    )
                    Spacer(Modifier.height(6.dp))
                    Button(
                        onClick = { if (aliasDraft.isNotBlank()) onSaveAlias(aliasDraft) },
                        colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.primary)
                    ) { Text("Update Name") }
                }
                item {
                    Text("Availability", fontWeight = FontWeight.SemiBold, style = MaterialTheme.typography.labelLarge)
                    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        listOf("available", "busy", "away", "offline").forEach { st ->
                            FilterChip(
                                selected = settings.status == st,
                                onClick = { onSetStatus(st) },
                                label = { Text(st.replaceFirstChar { it.uppercase() }, fontSize = 11.sp) }
                            )
                        }
                    }
                }
                item {
                    Text("Theme", fontWeight = FontWeight.SemiBold, style = MaterialTheme.typography.labelLarge)
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        listOf("light" to "Light", "dark" to "Dark", "system" to "System").forEach { (k, lbl) ->
                            FilterChip(
                                selected = settings.theme == k,
                                onClick = { onSetTheme(k) },
                                label = { Text(lbl) }
                            )
                        }
                    }
                }
                item {
                    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
                        Text("Auto-accept Favorites", modifier = Modifier.weight(1f))
                        Switch(checked = settings.autoAcceptFavorites, onCheckedChange = onToggleAutoAccept)
                    }
                    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
                        Text("LAN Auto-Discovery", modifier = Modifier.weight(1f))
                        Switch(checked = settings.discoveryEnabled, onCheckedChange = onToggleDiscovery)
                    }
                    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
                        Text("Notifications", modifier = Modifier.weight(1f))
                        Switch(checked = settings.soundEnabled, onCheckedChange = onToggleSound)
                    }
                }
                item {
                    Text("Device Identity", fontWeight = FontWeight.SemiBold, style = MaterialTheme.typography.labelLarge)
                    Text("ID: ${settings.deviceId}", style = MaterialTheme.typography.labelSmall)
                    Text("HTTPS Port: $boundPort", style = MaterialTheme.typography.labelSmall)
                    Text("TLS SHA-256: ${fingerprint.take(24)}...", style = MaterialTheme.typography.labelSmall)
                    Text("Downloads: $downloadPath", style = MaterialTheme.typography.labelSmall)
                }
            }
        },
        confirmButton = {
            TextButton(onClick = onDismiss) { Text("Done") }
        }
    )
}
