package com.shellfox.mobile.rust

import androidx.annotation.Keep

/** IDs come from Rust's registry. No native pointer crosses this boundary. */
@Keep
internal object RustBindings {
  init { System.loadLibrary("shellfox_android") }
  external fun abiVersion(): Int
  external fun create(cols: Int, rows: Int): Long
  external fun reset(handle: Long, cols: Int, rows: Int)
  external fun close(handle: Long)
  external fun submit(handle: Long, sequence: Long, bytes: ByteArray): Int
  external fun progress(handle: Long): LongArray
  external fun pump(handle: Long)
  external fun pumpFrame(handle: Long): ByteArray
  external fun resize(handle: Long, cols: Int, rows: Int)
  external fun input(handle: Long, json: ByteArray): ByteArray
  external fun inputEx(handle: Long, json: ByteArray): ByteArray
  external fun configure(handle: Long, json: ByteArray)
  external fun scroll(handle: Long, rows: Int)
  external fun select(handle: Long, kind: Int, sx: Int, sy: Int, ex: Int, ey: Int)
  external fun copy(handle: Long): ByteArray
  external fun hyperlink(handle: Long, x: Int, y: Int): ByteArray
  external fun fileLink(handle: Long, x: Int, y: Int): ByteArray
  external fun mouseTracking(handle: Long): Boolean
}
