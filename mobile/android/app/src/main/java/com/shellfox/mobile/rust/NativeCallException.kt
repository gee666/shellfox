package com.shellfox.mobile.rust

import androidx.annotation.Keep

@Keep
internal class NativeCallException(val status: Int) : IllegalStateException("Native operation failed.")

internal fun nativeCode(status: Int): String = when (status) {
  1 -> "BUSY"
  2 -> "INVALID"
  3 -> "STALE"
  4 -> "WRONG_THREAD"
  6 -> "PANIC"
  7 -> "EMPTY"
  8 -> "BUFFER"
  9 -> "CANCELLED"
  10 -> "NETWORK"
  11 -> "PROTOCOL"
  else -> "ENGINE"
}
