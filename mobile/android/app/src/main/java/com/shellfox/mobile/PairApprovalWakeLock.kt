package com.shellfox.mobile

import android.content.Context
import android.os.PowerManager

/** Finish pairing or withdrawal across screen-off; polls never keep an idle client awake. */
internal fun <T> withPairApprovalWakeLock(context: Context?, path: String, deadlineMs: Long, request: () -> T): T {
  val wakeLock = if (MobileEndpoint.fromPath(path)?.needsBackgroundCompletion == true) {
    context?.getSystemService(PowerManager::class.java)
      ?.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "Shellfox:PairApproval")
      ?.apply { acquire(deadlineMs.coerceIn(1L, 35000L)) }
  } else null
  return try { request() } finally { if (wakeLock?.isHeld == true) wakeLock.release() }
}
