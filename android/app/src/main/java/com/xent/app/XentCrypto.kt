package com.xent.app

import org.bouncycastle.asn1.x500.X500Name
import org.bouncycastle.cert.jcajce.JcaX509CertificateConverter
import org.bouncycastle.cert.jcajce.JcaX509v3CertificateBuilder
import org.bouncycastle.operator.jcajce.JcaContentSignerBuilder
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.math.BigInteger
import java.net.HttpURLConnection
import java.net.InetSocketAddress
import java.net.Proxy
import java.net.URL
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.MessageDigest
import java.security.SecureRandom
import java.security.cert.X509Certificate
import java.util.Date
import java.util.Locale
import javax.net.ssl.HostnameVerifier
import javax.net.ssl.HttpsURLConnection
import javax.net.ssl.KeyManagerFactory
import javax.net.ssl.SSLContext
import javax.net.ssl.TrustManager
import javax.net.ssl.X509TrustManager

data class XentTlsBundle(
    val serverContext: SSLContext,
    val clientContext: SSLContext,
    val fingerprintHex: String
)

object XentCrypto {
    private const val STORE_PASSWORD = "xent-lan-keystore"
    private const val KEY_ALIAS = "xent"

    private val trustAllManager = object : X509TrustManager {
        override fun checkClientTrusted(chain: Array<out X509Certificate>?, authType: String?) = Unit
        override fun checkServerTrusted(chain: Array<out X509Certificate>?, authType: String?) = Unit
        override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
    }

    val trustAllHostnameVerifier = HostnameVerifier { _, _ -> true }

    fun createClientSslContext(): SSLContext = SSLContext.getInstance("TLS").apply {
        init(null, arrayOf<TrustManager>(trustAllManager), SecureRandom())
    }

    fun loadOrCreateBundle(dataDir: File): XentTlsBundle {
        dataDir.mkdirs()
        val ksFile = File(dataDir, "xent_tls.p12")
        val ks = KeyStore.getInstance("PKCS12")
        val pwd = STORE_PASSWORD.toCharArray()

        if (ksFile.exists()) {
            val loaded = runCatching {
                FileInputStream(ksFile).use { ks.load(it, pwd) }
                ks.getCertificate(KEY_ALIAS) as? X509Certificate
            }.getOrNull()
            if (loaded != null) {
                return buildContexts(ks, pwd, loaded)
            }
        }

        // Generate fresh 2048-bit RSA self-signed cert valid 10 years
        val keyGen = KeyPairGenerator.getInstance("RSA")
        keyGen.initialize(2048, SecureRandom())
        val kp = keyGen.generateKeyPair()

        val now = System.currentTimeMillis()
        val notBefore = Date(now - 86_400_000L)
        val notAfter = Date(now + 10L * 365L * 86_400_000L)
        val subject = X500Name("CN=Xent LAN Node")
        val serial = BigInteger(64, SecureRandom()).abs().max(BigInteger.ONE)

        val builder = JcaX509v3CertificateBuilder(
            subject,
            serial,
            notBefore,
            notAfter,
            subject,
            kp.public
        )
        val signer = JcaContentSignerBuilder("SHA256withRSA").build(kp.private)
        val certHolder = builder.build(signer)
        val cert = JcaX509CertificateConverter().getCertificate(certHolder)

        ks.load(null, pwd)
        ks.setKeyEntry(KEY_ALIAS, kp.private, pwd, arrayOf(cert))
        FileOutputStream(ksFile).use { ks.store(it, pwd) }

        return buildContexts(ks, pwd, cert)
    }

    private fun buildContexts(ks: KeyStore, pwd: CharArray, cert: X509Certificate): XentTlsBundle {
        val kmf = KeyManagerFactory.getInstance(KeyManagerFactory.getDefaultAlgorithm())
        kmf.init(ks, pwd)
        val serverCtx = SSLContext.getInstance("TLS").apply {
            init(kmf.keyManagers, arrayOf<TrustManager>(trustAllManager), SecureRandom())
        }
        val sha = MessageDigest.getInstance("SHA-256").digest(cert.encoded)
        val hex = sha.joinToString("") { "%02x".format(Locale.US, it) }
        return XentTlsBundle(
            serverContext = serverCtx,
            clientContext = createClientSslContext(),
            fingerprintHex = hex
        )
    }

    fun openHttps(
        urlStr: String,
        sslContext: SSLContext,
        connectTimeoutMs: Int = 3500,
        readTimeoutMs: Int = 10000
    ): HttpsURLConnection {
        val url = URL(urlStr)
        val conn = url.openConnection(Proxy.NO_PROXY) as HttpsURLConnection
        conn.sslSocketFactory = sslContext.socketFactory
        conn.hostnameVerifier = trustAllHostnameVerifier
        conn.connectTimeout = connectTimeoutMs
        conn.readTimeout = readTimeoutMs
        return conn
    }

    fun postJson(
        ip: String,
        port: Int,
        path: String,
        jsonBody: String,
        sslContext: SSLContext,
        connectTimeoutMs: Int = 3000,
        readTimeoutMs: Int = 8000
    ): Pair<Int, String> {
        val conn = openHttps("https://$ip:$port$path", sslContext, connectTimeoutMs, readTimeoutMs)
        return try {
            conn.requestMethod = "POST"
            conn.doOutput = true
            conn.setRequestProperty("Content-Type", "application/json; charset=utf-8")
            val bytes = jsonBody.toByteArray(Charsets.UTF_8)
            conn.setFixedLengthStreamingMode(bytes.size)
            conn.outputStream.use { it.write(bytes) }
            val code = conn.responseCode
            val stream = if (code in 200..399) conn.inputStream else conn.errorStream
            val body = stream?.bufferedReader(Charsets.UTF_8)?.use { it.readText() } ?: ""
            code to body
        } finally {
            conn.disconnect()
        }
    }

    fun getText(
        ip: String,
        port: Int,
        path: String,
        sslContext: SSLContext,
        connectTimeoutMs: Int = 2500,
        readTimeoutMs: Int = 4000
    ): Pair<Int, String> {
        val conn = openHttps("https://$ip:$port$path", sslContext, connectTimeoutMs, readTimeoutMs)
        return try {
            conn.requestMethod = "GET"
            val code = conn.responseCode
            val stream = if (code in 200..399) conn.inputStream else conn.errorStream
            val body = stream?.bufferedReader(Charsets.UTF_8)?.use { it.readText() } ?: ""
            code to body
        } finally {
            conn.disconnect()
        }
    }

    fun tcpReachable(ip: String, port: Int, timeoutMs: Int = 450): Boolean = runCatching {
        java.net.Socket().use { sock ->
            sock.connect(InetSocketAddress(ip, port), timeoutMs)
            true
        }
    }.getOrDefault(false)
}
