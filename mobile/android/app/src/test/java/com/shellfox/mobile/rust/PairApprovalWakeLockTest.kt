package com.shellfox.mobile.rust

import com.shellfox.mobile.withPairApprovalWakeLock
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowPowerManager

@RunWith(RobolectricTestRunner::class)
@Config(manifest = Config.NONE, sdk = [24], application = android.app.Application::class)
class PairApprovalWakeLockTest {
  @Test fun pairingAndDisconnectHoldPowerOnlyDuringRequestAndReleaseOnFailure() {
    val context = RuntimeEnvironment.getApplication()
    for (path in listOf("/v1/pair", "/v2/pair", "/v1/disconnect", "/v2/disconnect")) {
      assertEquals("ok", withPairApprovalWakeLock(context, path, 1000) {
        assertTrue(ShadowPowerManager.getLatestWakeLock().isHeld)
        "ok"
      })
      assertFalse(ShadowPowerManager.getLatestWakeLock().isHeld)
      assertThrows(IllegalStateException::class.java) {
        withPairApprovalWakeLock(context, path, 1000) { error("fixture") }
      }
      assertFalse(ShadowPowerManager.getLatestWakeLock().isHeld)
    }
  }
  @Test fun ordinaryRequestsDoNotAcquireAWakeLock() {
    val before = ShadowPowerManager.getLatestWakeLock()
    for (path in listOf("/v1/events", "/v2/events")) {
      val value = withPairApprovalWakeLock(RuntimeEnvironment.getApplication(), path, 1000) { "fixture" }
      assertEquals("fixture", value)
      assertSame(before, ShadowPowerManager.getLatestWakeLock())
    }
  }
}
