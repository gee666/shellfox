package com.shellfox.mobile.rust

/** Snapshot queried natively for the exact live ticket, never event geometry. */
internal sealed interface ResetGrid {
  data class Host(val cols: Int, val rows: Int) : ResetGrid
  data object FenceOnly : ResetGrid
  data object Stale : ResetGrid
  companion object {
    fun decode(value: IntArray): ResetGrid {
      require(value.size == 3)
      val status = value[0]
      val cols = value[1]
      val rows = value[2]
      if (status == 0) {
        require(cols in 1..512 && rows in 1..512 && cols * rows <= 65536)
        return Host(cols, rows)
      }
      require(cols == 0 && rows == 0)
      return when (status) {
        3 -> Stale
        7 -> FenceOnly
        else -> error("Native reset grid failed")
      }
    }
  }
}
