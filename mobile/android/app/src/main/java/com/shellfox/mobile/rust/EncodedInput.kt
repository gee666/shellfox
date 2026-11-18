package com.shellfox.mobile.rust

/** Rust-derived metadata travels with the owned bytes through every BUSY retry. */
internal data class EncodedInput(val bytes: ByteArray, val userInitiated: Boolean) {
  companion object {
    fun decode(packet: ByteArray): EncodedInput? {
      if (packet.isEmpty()) return null
      require(packet.size in 2..65537 && packet[0].toInt() in 0..1)
      return EncodedInput(packet.copyOfRange(1, packet.size), packet[0] == 1.toByte())
    }
  }
}
