package com.shellfox.mobile.rust

internal class NativeResponseTooLarge : RuntimeException()
internal class NativeInvalidHttpReply : RuntimeException()

/** Only a received Go HTTP response may construct this value. No fallback decisions here. */
internal data class NativeHttpReply(val status: Int, val bytes: ByteArray) {
  companion object {
    fun received(status: Long, body: String, limit: Long): NativeHttpReply {
      if (status !in 100L..599L || limit !in 1L..8_388_608L) throw NativeInvalidHttpReply()
      val bytes = body.toByteArray(Charsets.UTF_8)
      if (bytes.size > limit) throw NativeResponseTooLarge()
      if (status !in 200L..299L && bytes.isNotEmpty()) throw NativeInvalidHttpReply()
      return NativeHttpReply(status.toInt(), bytes)
    }
  }
}
