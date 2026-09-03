package com.droplink.app.crypto

import java.io.InputStream
import java.nio.ByteBuffer
import java.security.MessageDigest
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec

object AndroidCrypto {

    /**
     * Computes the SHA-256 hex string of a stream asynchronously.
     */
    fun computeSha256(inputStream: InputStream): String {
        val digest = MessageDigest.getInstance("SHA-256")
        val buffer = ByteArray(64 * 1024)
        var read: Int
        while (inputStream.read(buffer).also { read = it } != -1) {
            digest.update(buffer, 0, read)
        }
        val hash = digest.digest()
        return hash.joinToString("") { "%02x".format(it) }
    }

    /**
     * Computes the deterministic 6-digit Short Authentication String (SAS) PIN
     * matching the DropLink core wire specification.
     */
    fun computeSasPin(fingerprintA: String, fingerprintB: String): String {
        val sorted = listOf(fingerprintA.uppercase(), fingerprintB.uppercase()).sorted()
        val salt = "DROPLINK_SAS_V1".toByteArray(Charsets.UTF_8)
        val ikm = "${sorted[0]}:${sorted[1]}".toByteArray(Charsets.UTF_8)

        // HKDF-Extract: PRK = HMAC-Hash(salt, IKM)
        val mac = Mac.getInstance("HmacSHA256")
        mac.init(SecretKeySpec(salt, "HmacSHA256"))
        val prk = mac.doFinal(ikm)

        // HKDF-Expand: OKM = HMAC-Hash(PRK, info || 0x01)
        val expandMac = Mac.getInstance("HmacSHA256")
        expandMac.init(SecretKeySpec(prk, "HmacSHA256"))
        expandMac.update("droplink-sas-pin".toByteArray(Charsets.UTF_8))
        expandMac.update(1.toByte())
        val okm = expandMac.doFinal()

        // Take 4 bytes
        val buffer = ByteBuffer.wrap(okm, 0, 4)
        val value = (buffer.int.toLong() and 0xFFFFFFFFL) % 1_000_000

        val part1 = (value / 1000).toInt()
        val part2 = (value % 1000).toInt()
        return "%03d %03d".format(part1, part2)
    }
}
