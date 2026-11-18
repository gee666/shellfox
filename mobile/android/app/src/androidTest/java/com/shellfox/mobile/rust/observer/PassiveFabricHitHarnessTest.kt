package com.shellfox.mobile.rust.observer

import android.app.Activity
import android.app.Application
import android.content.Intent
import android.graphics.Rect
import android.os.Bundle
import android.os.SystemClock
import android.view.accessibility.AccessibilityNodeInfo
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.shellfox.mobile.MainActivity
import org.json.JSONObject
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.CountDownLatch
import java.util.concurrent.atomic.AtomicReference

/** Start instrumentation before ANY app Go/PAIR/view. Never acts on the UI. */
@RunWith(AndroidJUnit4::class)
class PassiveFabricHitHarnessTest {
  @Test fun observeRootOwnedKeyboardPointWithoutDispatch() {
    val instrumentation = InstrumentationRegistry.getInstrumentation()
    val arguments = InstrumentationRegistry.getArguments()
    assumeTrue(arguments.getString("rootOwnedObservation") == "explicit-fresh-scope")
    require(arguments.getString("class") == javaClass.name + "#observeRootOwnedKeyboardPointWithoutDispatch")
    require(instrumentation.targetContext.packageName == "com.shellfox.mobile.terminaltest")
    val origin = requireNotNull(arguments.getString("controlOrigin"))
    require(origin.toByteArray(Charsets.UTF_8).size in 1..2048) // Product Go parser remains authoritative.
    val seconds = requireNotNull(arguments.getString("observationSeconds")).toLong()
    require(seconds in 10..900)
    val app = instrumentation.targetContext.applicationContext as Application
    val tracker = LifetimeObservation()
    instrumentation.runOnMainSync { app.registerActivityLifecycleCallbacks(tracker) }
    var observerFailed = false
    try {
      // The target process is inert until this existing product launch. No fake
      // peer, SDK state, approval or parser fixture is installed by this test.
      val selection = JSONObject().put("profile", "manager-v2").put("engine",
        JSONObject().put("engine", "rust-native").put("accepted",
          JSONObject().put("android", true).put("ios", false))).toString()
      val intent = Intent(instrumentation.targetContext, MainActivity::class.java)
        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        .putExtra("shellfoxProfileSelection", selection).putExtra("shellfoxControlURL", origin)
      val launched = instrumentation.startActivitySync(intent)
      require(launched is MainActivity)
      tracker.original.set(launched)
      val end = SystemClock.uptimeMillis() + seconds * 1000
      var originalPoint: Rect? = null
      while (tracker.destroyed.count != 0L && SystemClock.uptimeMillis() < end) {
        val ax = readKeyboardBounds(instrumentation.uiAutomation.rootInActiveWindow)
        var result = HitObservation(axKeyboardUnique = ax.unique)
        instrumentation.runOnMainSync {
          if (!launched.isDestroyed) result = FabricHitProbe.sample(launched, ax, originalPoint ?: ax.bounds)
        }
        if (originalPoint == null && ax.unique && result.nativeKeyboardUnique && result.nativeTerminalUnique && result.rootCoordinatesSupported)
          originalPoint = ax.bounds?.let(::Rect)
        instrumentation.sendStatus(0, Bundle().apply { putString("hitObservation", result.json().put("probePointFrozen", originalPoint != null).toString()) })
        SystemClock.sleep(1000)
      }
      if (tracker.destroyed.count != 0L) {
        instrumentation.sendStatus(0, Bundle().apply {
          putString("hitObservation", JSONObject().put("cleanupRequired", true)
            .put("observationWindowEnded", true).put("activityDestroyed", false).toString())
        })
        // Do not finish instrumentation into an active owned lease. Root must
        // use real product Stop/disabled, then normal Activity closure. This
        // latch is not a Go/lease cleanup receipt and has no automatic action.
      }
    } catch (_: Exception) {
      observerFailed = true
      instrumentation.sendStatus(0, Bundle().apply { putString("hitObservation", JSONObject().put("observerFailed", true).toString()) })
    } catch (_: LinkageError) {
      observerFailed = true
      instrumentation.sendStatus(0, Bundle().apply { putString("hitObservation", JSONObject().put("observerFailed", true).toString()) })
    } finally {
      if (tracker.original.get() != null && tracker.destroyed.count != 0L) {
        instrumentation.sendStatus(0, Bundle().apply {
          putString("hitObservation", JSONObject().put("cleanupRequired", true).put("activityDestroyed", false).toString())
        })
        tracker.destroyed.await() // Also retain on observer/test failure; never tear down an active context.
      }
      instrumentation.runOnMainSync { app.unregisterActivityLifecycleCallbacks(tracker) }
      instrumentation.sendStatus(0, Bundle().apply {
        putString("hitObservation", JSONObject().put("activityDestroyed", tracker.original.get() != null && tracker.destroyed.count == 0L)
          .put("goOrLeaseCleanupCertified", false).toString())
      })
    }
    if (observerFailed) throw AssertionError("Read-only observer failed; unredacted details suppressed.")
  }
  private fun readKeyboardBounds(root: AccessibilityNodeInfo?): AxKeyboard {
    if (root == null) return AxKeyboard()
    val waiting = ArrayDeque<AccessibilityNodeInfo>(); waiting.add(root)
    var visited = 0; var found: AxKeyboard? = null; var duplicate = false
    while (waiting.isNotEmpty() && visited++ < 512) {
      val node = waiting.removeFirst()
      // No node.text access: terminal cells, command text and keys stay unread.
      if (node.packageName?.toString() == "com.shellfox.mobile.terminaltest" &&
        node.className?.toString() == "android.widget.Button" && node.contentDescription?.toString() == "Keyboard" &&
        node.isVisibleToUser && node.isEnabled && node.isClickable) {
        if (found != null) duplicate = true
        else found = AxKeyboard(Rect().also { node.getBoundsInScreen(it) }, true,
          node.isEnabled, node.isClickable, node.isFocusable, node.isFocused, node.isSelected)
      }
      for (index in 0 until node.childCount) node.getChild(index)?.let(waiting::addLast)
    }
    return if (duplicate || waiting.isNotEmpty()) AxKeyboard() else found ?: AxKeyboard()
  }
  private class LifetimeObservation : Application.ActivityLifecycleCallbacks {
    val original = AtomicReference<Activity?>()
    val destroyed = CountDownLatch(1)
    override fun onActivityDestroyed(activity: Activity) { if (activity === original.get()) destroyed.countDown() }
    override fun onActivityCreated(activity: Activity, savedInstanceState: Bundle?) { if (activity is MainActivity) original.compareAndSet(null, activity) }
    override fun onActivityStarted(activity: Activity) = Unit
    override fun onActivityResumed(activity: Activity) = Unit
    override fun onActivityPaused(activity: Activity) = Unit
    override fun onActivityStopped(activity: Activity) = Unit
    override fun onActivitySaveInstanceState(activity: Activity, outState: Bundle) = Unit
  }
}
