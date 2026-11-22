package com.shellfox.mobile.rust

import androidx.annotation.Keep

@Keep
internal object ClientBindings {
  init { System.loadLibrary("shellfox_android") }
  external fun create(peer: ByteArray, clientId: ByteArray): Long
  external fun managerAbiVersion(): Int
  external fun createManager(peer: ByteArray, clientId: ByteArray): Long
  external fun discoverManager(client: Long): Long
  external fun pairManager(client: Long, attempt: ByteArray): Long
  external fun reconnectManager(client: Long, clientId: ByteArray)
  external fun inputManager(client: Long, terminal: Long, json: ByteArray, userInitiated: Boolean): Int
  external fun observerAbiVersion(): Int
  external fun createObserver(peer: ByteArray, clientId: ByteArray): Long
  external fun discoverObserver(client: Long, offerLegacy: Boolean): Long
  external fun pairObserver(client: Long, attempt: ByteArray): Long
  external fun reconnectObserver(client: Long, clientId: ByteArray)
  external fun profile(client: Long): IntArray
  external fun start(client: Long, kind: Int, json: ByteArray?): Long
  external fun cancelOperation(client: Long, operation: Long)
  external fun bind(client: Long, tab: ByteArray, terminal: Long)
  external fun unbind(client: Long, terminal: Long)
  external fun consumed(client: Long, terminal: Long): Int
  external fun recoveryReady(client: Long, terminal: Long): Boolean
  external fun resetTicket(client: Long, terminal: Long): Long
  external fun resetGrid(client: Long, terminal: Long, ticket: Long): IntArray
  external fun resetDone(client: Long, terminal: Long, ticket: Long): Int
  external fun resize(client: Long, terminal: Long, cols: Int, rows: Int): Int
  external fun write(client: Long, terminal: Long, bytes: ByteArray, userInitiated: Boolean): Int
  external fun setWaker(client: Long)
  external fun dispatch(client: Long, transport: Class<*>)
  external fun close(client: Long, transport: Class<*>)
  external fun complete(client: Long, generation: Long, request: Long, outcome: Int, bytes: ByteArray): Int
  external fun completeHttp(client: Long, generation: Long, request: Long, outcome: Int, httpStatus: Int, bytes: ByteArray): Int
  external fun readEvent(client: Long): ByteArray?

  /** Called by Rust from any thread; coalesced until the next dispatch. */
  @JvmStatic fun wake(client: Long) = TerminalCoordinator.wakeClient(client)
}

internal fun nativeId(value: String): Long {
  require(value.matches(Regex("[1-9][0-9]{0,19}")))
  val parsed = java.math.BigInteger(value)
  require(parsed.signum() > 0 && parsed.bitLength() <= 64)
  return parsed.toLong()
}
// java.lang.Long's unsigned helpers require API 26. Keep the ABI lossless on 24.
internal fun idString(value: Long): String = if (value >= 0) value.toString()
  else java.math.BigInteger.valueOf(value).add(java.math.BigInteger.ONE.shiftLeft(64)).toString()
internal fun compareNativeIds(left: Long, right: Long): Int =
  (left xor Long.MIN_VALUE).compareTo(right xor Long.MIN_VALUE)
