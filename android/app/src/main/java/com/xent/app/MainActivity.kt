package com.xent.app

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.os.Environment
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import androidx.core.content.FileProvider
import com.xent.app.ui.XentAppScreen
import java.io.File
import java.net.URLConnection

class MainActivity : ComponentActivity() {
    companion object {
        private const val CHANNEL_ID = "xent_events"
    }

    private lateinit var engine: XentEngine

    private val notifPermLauncher = registerForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) { _ -> }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val defaultAlias = Build.MODEL?.takeIf { it.isNotBlank() } ?: "Android Device"
        val dataDir = File(filesDir, "xent")
        val downloadDir = getExternalFilesDir(Environment.DIRECTORY_DOWNLOADS)
            ?.let { File(it, "Xent") }
            ?: File(filesDir, "downloads")

        engine = XentEngine(
            dataDir = dataDir,
            downloadDir = downloadDir,
            defaultAlias = defaultAlias,
            context = applicationContext,
            enableUdpDiscovery = true
        )
        setupNotificationChannel()
        engine.onIncomingNotify = { title, body ->
            showNotification(title, body)
        }
        engine.start()

        setContent {
            XentAppScreen(
                engine = engine,
                onOpenReceivedFile = { path -> openLocalFile(path) }
            )
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        if (isFinishing) {
            engine.stop()
        }
    }

    private fun setupNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val mgr = getSystemService(NotificationManager::class.java)
            mgr?.createNotificationChannel(
                NotificationChannel(
                    CHANNEL_ID,
                    "Xent Messages & Transfers",
                    NotificationManager.IMPORTANCE_DEFAULT
                )
            )
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            if (ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS)
                != PackageManager.PERMISSION_GRANTED
            ) {
                notifPermLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
            }
        }
    }

    private fun showNotification(title: String, body: String) {
        if (!engine.store.getSettings().soundEnabled) return
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS)
            != PackageManager.PERMISSION_GRANTED
        ) {
            return
        }
        val notif = NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.mipmap.ic_launcher)
            .setContentTitle(title)
            .setContentText(body)
            .setAutoCancel(true)
            .build()
        runCatching {
            NotificationManagerCompat.from(this).notify((System.currentTimeMillis() % 100_000).toInt(), notif)
        }
    }

    private fun openLocalFile(path: String): String? {
        val file = File(path)
        if (!file.exists()) return "File not found on device"
        return runCatching {
            val uri = FileProvider.getUriForFile(this, "${packageName}.fileprovider", file)
            val mime = URLConnection.guessContentTypeFromName(file.name) ?: "*/*"
            val intent = Intent(Intent.ACTION_VIEW).apply {
                setDataAndType(uri, mime)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }
            startActivity(Intent.createChooser(intent, "Open ${file.name}"))
            null
        }.getOrElse { it.message ?: "No app available to open file" }
    }
}
