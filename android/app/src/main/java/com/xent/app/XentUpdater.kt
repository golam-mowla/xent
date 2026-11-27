package com.xent.app

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.content.FileProvider
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.io.File
import java.io.FileOutputStream
import java.net.HttpURLConnection
import java.net.URL

data class AndroidUpdateInfo(
    val currentVersion: String,
    val latestVersion: String,
    val hasUpdate: Boolean,
    val releaseName: String,
    val releaseNotes: String,
    val releaseUrl: String,
    val downloadUrl: String?,
    val apkName: String?,
    val apkSize: Long?
)

object XentUpdater {
    private const val REPO_OWNER = "golam-mowla"
    private const val REPO_NAME = "xent"

    fun isNewerVersion(latest: String, current: String): Boolean {
        fun parse(v: String): List<Int> = v.trim()
            .trimStart('v', 'V')
            .split('.')
            .mapNotNull { seg -> seg.takeWhile { it.isDigit() }.toIntOrNull() }

        val l = parse(latest)
        val c = parse(current)
        val maxLen = maxOf(l.size, c.size)
        for (i in 0 until maxLen) {
            val lv = l.getOrElse(i) { 0 }
            val cv = c.getOrElse(i) { 0 }
            if (lv > cv) return true
            if (lv < cv) return false
        }
        return false
    }

    fun parseTagFromReleaseUrl(url: String): String? {
        val marker = "/releases/tag/"
        val idx = url.indexOf(marker)
        if (idx < 0) return null
        val tail = url.substring(idx + marker.length)
        val tag = tail.takeWhile { it != '?' && it != '#' && it != '/' }.trim()
        return tag.ifEmpty { null }
    }

    fun parseApkAssetFromExpandedHtml(html: String, tag: String): Pair<String, String>? {
        val prefix = "/$REPO_OWNER/$REPO_NAME/releases/download/$tag/"
        var pos = 0
        while (true) {
            val found = html.indexOf(prefix, pos)
            if (found < 0) break
            val tail = html.substring(found)
            val end = tail.indexOfAny(charArrayOf('"', '\'', '<', ' ', '\n', '\r'))
            val relUrl = if (end > 0) tail.substring(0, end) else tail
            val fileName = relUrl.substringAfterLast('/')
            if (fileName.lowercase().endsWith(".apk")) {
                return "https://github.com$relUrl" to fileName
            }
            pos = found + prefix.length
        }
        return null
    }

    suspend fun checkForUpdates(currentVersion: String): Result<AndroidUpdateInfo> = withContext(Dispatchers.IO) {
        runCatching {
            val apiUrl = "https://api.github.com/repos/$REPO_OWNER/$REPO_NAME/releases/latest"
            val apiConn = (URL(apiUrl).openConnection() as HttpURLConnection).apply {
                setRequestProperty("User-Agent", "Xent-Android/$currentVersion")
                setRequestProperty("Accept", "application/vnd.github+json")
                connectTimeout = 10_000
                readTimeout = 12_000
            }
            try {
                if (apiConn.responseCode in 200..299) {
                    val body = apiConn.inputStream.bufferedReader().use { it.readText() }
                    val json = JSONObject(body)
                    val tag = json.optString("tag_name")
                    val latest = tag.trimStart('v', 'V')
                    val assets = json.optJSONArray("assets")
                    var dlUrl: String? = null
                    var apkName: String? = null
                    var apkSize: Long? = null
                    if (assets != null) {
                        for (i in 0 until assets.length()) {
                            val a = assets.getJSONObject(i)
                            val name = a.optString("name")
                            if (name.lowercase().endsWith(".apk")) {
                                dlUrl = a.optString("browser_download_url")
                                apkName = name
                                apkSize = a.optLong("size", 0L).takeIf { it > 0L }
                                break
                            }
                        }
                    }
                    return@runCatching AndroidUpdateInfo(
                        currentVersion = currentVersion,
                        latestVersion = latest,
                        hasUpdate = isNewerVersion(latest, currentVersion),
                        releaseName = json.optString("name", tag).ifBlank { tag },
                        releaseNotes = json.optString("body", ""),
                        releaseUrl = json.optString("html_url", "https://github.com/$REPO_OWNER/$REPO_NAME/releases"),
                        downloadUrl = dlUrl,
                        apkName = apkName,
                        apkSize = apkSize
                    )
                }
            } finally {
                apiConn.disconnect()
            }

            // Fallback to web redirect when GitHub REST API hits 403/429 rate limits
            val webUrl = "https://github.com/$REPO_OWNER/$REPO_NAME/releases/latest"
            val webConn = (URL(webUrl).openConnection() as HttpURLConnection).apply {
                instanceFollowRedirects = true
                setRequestProperty("User-Agent", "Xent-Android/$currentVersion")
                connectTimeout = 10_000
                readTimeout = 12_000
            }
            val finalUrl: String = try {
                webConn.inputStream.use { }
                webConn.url.toString()
            } finally {
                webConn.disconnect()
            }
            val tag = parseTagFromReleaseUrl(finalUrl)
                ?: error("No published GitHub release found yet")
            val latest = tag.trimStart('v', 'V')

            var dlUrl: String? = null
            var apkName: String? = null
            val expConn = (URL("https://github.com/$REPO_OWNER/$REPO_NAME/releases/expanded_assets/$tag")
                .openConnection() as HttpURLConnection).apply {
                setRequestProperty("User-Agent", "Xent-Android/$currentVersion")
                connectTimeout = 10_000
                readTimeout = 12_000
            }
            try {
                if (expConn.responseCode in 200..299) {
                    val html = expConn.inputStream.bufferedReader().use { it.readText() }
                    parseApkAssetFromExpandedHtml(html, tag)?.let { (u, n) ->
                        dlUrl = u
                        apkName = n
                    }
                }
            } finally {
                expConn.disconnect()
            }

            AndroidUpdateInfo(
                currentVersion = currentVersion,
                latestVersion = latest,
                hasUpdate = isNewerVersion(latest, currentVersion),
                releaseName = tag,
                releaseNotes = "",
                releaseUrl = finalUrl,
                downloadUrl = dlUrl,
                apkName = apkName,
                apkSize = null
            )
        }
    }

    suspend fun downloadApk(
        context: Context,
        downloadUrl: String,
        onProgress: (Float) -> Unit
    ): Result<File> = withContext(Dispatchers.IO) {
        runCatching {
            val updatesDir = File(context.filesDir, "updates").apply { mkdirs() }
            val outFile = File(updatesDir, "xent-update.apk")
            if (outFile.exists()) outFile.delete()

            val conn = (URL(downloadUrl).openConnection() as HttpURLConnection).apply {
                instanceFollowRedirects = true
                setRequestProperty("User-Agent", "Xent-Android/${BuildConfig.VERSION_NAME}")
                connectTimeout = 15_000
                readTimeout = 60_000
            }
            try {
                if (conn.responseCode !in 200..299) {
                    error("Download failed with HTTP ${conn.responseCode}")
                }
                val total = conn.contentLengthLong
                conn.inputStream.use { input ->
                    FileOutputStream(outFile).use { output ->
                        val buf = ByteArray(16 * 1024)
                        var downloaded = 0L
                        while (true) {
                            val r = input.read(buf)
                            if (r <= 0) break
                            output.write(buf, 0, r)
                            downloaded += r
                            if (total > 0L) {
                                onProgress((downloaded.toFloat() / total.toFloat()).coerceIn(0f, 1f))
                            }
                        }
                    }
                }
            } finally {
                conn.disconnect()
            }
            onProgress(1f)
            outFile
        }
    }

    fun launchApkInstaller(context: Context, apkFile: File): String? {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O &&
            !context.packageManager.canRequestPackageInstalls()
        ) {
            val permIntent = Intent(
                Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                Uri.parse("package:${context.packageName}")
            ).apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            context.startActivity(permIntent)
            return "Allow 'Install unknown apps' for Xent, then tap Install Update again."
        }
        val uri = FileProvider.getUriForFile(context, "${context.packageName}.fileprovider", apkFile)
        val intent = Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(uri, "application/vnd.android.package-archive")
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
        }
        context.startActivity(intent)
        return null
    }
}
