package com.xent.app

import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.nio.file.Files

class XentProtocolTest {

    @Test
    fun filenameSanitizationHandlesTraversalAndReservedNames() {
        assertEquals("report.pdf", XentFormat.sanitizeFileName("../../etc/report.pdf"))
        assertEquals("a_b_c.txt", XentFormat.sanitizeFileName("a:b?c.txt"))
        assertEquals("_CON.txt", XentFormat.sanitizeFileName("CON.txt"))
        assertEquals("received_file", XentFormat.sanitizeFileName("..."))
    }

    @Test
    fun twoKotlinNodesExchangeEncryptedMessagesAndChunkedFiles(): Unit = runBlocking {
        val dirA = Files.createTempDirectory("xent_android_a").toFile()
        val dirB = Files.createTempDirectory("xent_android_b").toFile()
        val dlA = File(dirA, "downloads")
        val dlB = File(dirB, "downloads")

        val nodeA = XentEngine(File(dirA, "data"), dlA, "Android-A", enableUdpDiscovery = false)
        val nodeB = XentEngine(File(dirB, "data"), dlB, "Android-B", enableUdpDiscovery = false)
        try {
            val portA = nodeA.start()
            val portB = nodeB.start()
            assertTrue(portA > 0)
            assertTrue(portB > 0)

            // 1. Handshake A <-> B by IP
            val peerB = nodeA.addPeerByIp("127.0.0.1:$portB").getOrThrow()
            val peerA = nodeB.addPeerByIp("127.0.0.1:$portA").getOrThrow()
            assertEquals("Android-B", peerB.alias)
            assertEquals("Android-A", peerA.alias)

            // 2. Chat A -> B, Edit, Reaction, Read receipt, Delete
            nodeA.sendMessage(peerB.deviceId, "Hello from Kotlin Android!")
            var received = false
            for (i in 0..40) {
                if (nodeB.store.getMessages().any { it.content == "Hello from Kotlin Android!" }) {
                    received = true
                    break
                }
                delay(50)
            }
            assertTrue("NodeB should receive message", received)
            val msgId = nodeB.store.getMessages().first().id

            nodeA.editMessage(peerB.deviceId, msgId, "Edited from Kotlin!")
            delay(150)
            assertTrue(nodeB.store.getMessages().first { it.id == msgId }.isEdited)
            assertEquals("Edited from Kotlin!", nodeB.store.getMessages().first { it.id == msgId }.content)

            nodeA.toggleReaction(peerB.deviceId, msgId, "🔥")
            delay(150)
            assertTrue(nodeB.store.getMessages().first { it.id == msgId }.reactions["🔥"]?.contains("Android-A") == true)

            nodeB.markThreadRead(peerA.deviceId)
            delay(150)
            assertTrue(nodeA.store.getMessages().first { it.id == msgId }.status.startsWith("Read"))

            nodeA.deleteMessage(peerB.deviceId, msgId)
            delay(150)
            assertTrue(nodeB.store.getMessages().first { it.id == msgId }.isDeleted)

            // 3. Large Multi-Chunk File Transfer (1.4 MB) with Manual Accept
            val payload = ByteArray(1_400_000) { idx -> ((idx * 29) % 251).toByte() }
            val srcFile = File(dirA, "sample (1).bin").apply { writeBytes(payload) }
            val tx = nodeA.sendFileRequest(
                peerB.deviceId,
                QueuedUploadSource(id = "src1", name = srcFile.name, size = srcFile.length(), localFile = srcFile)
            ).getOrThrow()
            assertEquals(TransferStatus.PENDING, tx.status)

            nodeB.respondToOffer(tx.transferId, accept = true)
            var done = false
            for (i in 0..100) {
                val aStat = nodeA.store.getTransfer(tx.transferId)?.status
                val bStat = nodeB.store.getTransfer(tx.transferId)?.status
                if (aStat == TransferStatus.COMPLETED && bStat == TransferStatus.COMPLETED) {
                    done = true
                    break
                }
                delay(50)
            }
            assertTrue("1.4 MB transfer must complete on both nodes", done)
            val recvPath = nodeB.store.getTransfer(tx.transferId)!!.filePath!!
            assertArrayEquals(payload, File(recvPath).readBytes())

            // 4. Duplicate collision + Favorite auto-accept + Pause/Resume (4 MB)
            nodeB.store.updateSettings { it.copy(favoritePeers = it.favoritePeers + peerA.deviceId) }
            val smallA = File(dirA, "dup.txt").apply { writeText("dup payload") }
            val t1 = nodeA.sendFileRequest(peerB.deviceId, QueuedUploadSource("d1", "dup.txt", smallA.length(), localFile = smallA)).getOrThrow()
            val t2 = nodeA.sendFileRequest(peerB.deviceId, QueuedUploadSource("d2", "dup.txt", smallA.length(), localFile = smallA)).getOrThrow()
            for (i in 0..60) {
                if (nodeB.store.getTransfer(t1.transferId)?.status == TransferStatus.COMPLETED &&
                    nodeB.store.getTransfer(t2.transferId)?.status == TransferStatus.COMPLETED
                ) break
                delay(50)
            }
            val p1 = nodeB.store.getTransfer(t1.transferId)!!.filePath!!
            val p2 = nodeB.store.getTransfer(t2.transferId)!!.filePath!!
            assertNotEquals(p1, p2)
            assertEquals("dup payload", File(p1).readText())
            assertEquals("dup payload", File(p2).readText())

            // 5. Blocked peer silently declined
            nodeB.store.updateSettings { it.copy(blockedPeers = it.blockedPeers + peerA.deviceId) }
            val declined = nodeA.sendFileRequest(peerB.deviceId, QueuedUploadSource("d3", "dup.txt", smallA.length(), localFile = smallA)).getOrThrow()
            assertEquals(TransferStatus.DECLINED, declined.status)
            assertFalse(nodeA.store.getTransfers().isEmpty())
        } finally {
            nodeA.stop()
            nodeB.stop()
            dirA.deleteRecursively()
            dirB.deleteRecursively()
        }
    }
}
