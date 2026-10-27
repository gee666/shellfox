package com.shellfox.mobile

import android.os.PowerManager
import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import go.Seq
import mobile.Mobile
import java.io.File
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit
import java.util.concurrent.SynchronousQueue
import java.util.concurrent.RejectedExecutionException

/** Promise strings contain the Go library's bounded JSON, never SDK diagnostics. */
class ShellfoxNetworkModule(context: ReactApplicationContext) : ReactContextBaseJavaModule(context) {
  private val control = executor(1)
  // No operation queue: a queued terminal mutation could run after JS timed out.
  private val requests = immediateExecutor(12, "ShellfoxNetworkRequest")
  // Polls cannot occupy operation workers. Idle poll threads expire after a minute.
  private val polls = immediateExecutor(64, "ShellfoxNetworkPoll")
  @Volatile private var invalidated = false
  private val networkProvider = AndroidNetworkProvider(context)

  init {
    Seq.setContext(context.applicationContext)
    Mobile.setNetworkProvider(networkProvider)
    networkProvider.register()
  }

  private val startErrors = setOf(
    "Invalid state directory or hostname", "Network already started with another configuration",
    "Cannot create node directory", "Invalid node directory", "Cannot secure node directory",
    "Cannot create node state", "Invalid node state file", "Cannot secure node state",
    "Cannot read node state", "Invalid node state data", "Cannot initialize embedded Tailscale",
    "Cannot read Android network interfaces", "Cannot initialize network API", "Cannot read network status",
    "Cannot configure embedded runtime", "Cannot initialize network health",
  )

  override fun getName() = "ShellfoxNetwork"

  private fun immediateExecutor(limit: Int, name: String) = ThreadPoolExecutor(
    0, limit, 60L, TimeUnit.SECONDS, SynchronousQueue<Runnable>(),
    { task -> Thread(task, name).apply { isDaemon = true } },
    ThreadPoolExecutor.AbortPolicy(),
  )

  private fun executor(threads: Int) = ThreadPoolExecutor(
    threads, threads, 0L, TimeUnit.MILLISECONDS, ArrayBlockingQueue<Runnable>(32),
    { task -> Thread(task, "ShellfoxNetwork").apply { isDaemon = true } },
    ThreadPoolExecutor.AbortPolicy(),
  )

  private fun submit(pool: ThreadPoolExecutor, promise: Promise, start: Boolean = false, action: () -> Any?) {
    if (invalidated) {
      promise.reject("NETWORK_STOPPED", "Network module is unavailable.")
      return
    }
    try {
      pool.execute {
        try {
          if (invalidated) promise.reject("NETWORK_STOPPED", "Network module is unavailable.")
          else promise.resolve(action())
        } catch (error: Exception) {
          // Do not expose native exceptions, URLs, keys, paths, or response bodies.
          val safe = if (start && error.message in startErrors) error.message else "Network operation failed."
          promise.reject("NETWORK_ERROR", safe)
        }
      }
    } catch (error: RejectedExecutionException) {
      promise.reject("NETWORK_BUSY", "Too many pending network operations.")
    }
  }

  @ReactMethod
  fun start(hostname: String, promise: Promise) = submit(control, promise, start = true) {
    // Unlike filesDir, this directory is excluded from Android auto-backup.
    val directory = File(reactApplicationContext.noBackupFilesDir, "shellfox-tailscale")
    Mobile.start(directory.absolutePath, hostname)
    Mobile.status()
  }

  @ReactMethod
  fun status(promise: Promise) = submit(control, promise) { Mobile.status() }

  @ReactMethod
  fun login(authKey: String, promise: Promise) = submit(control, promise) {
    Mobile.login(authKey)
    Mobile.status()
  }

  @ReactMethod
  fun peers(promise: Promise) = submit(requests, promise) { Mobile.peers() }

  @ReactMethod
  fun hosts(promise: Promise) = submit(requests, promise) { Mobile.hosts() }

  @ReactMethod
  fun request(peerId: String, path: String, body: String, promise: Promise) = submit(if (path == "/v1/events") polls else requests, promise) {
    // Finish a foreground-started approval POST across screen-off. Never keep an idle client awake.
    val wakeLock = if (path == "/v1/pair") {
      reactApplicationContext.getSystemService(PowerManager::class.java)
        ?.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "Shellfox:PairApproval")
        ?.apply { acquire(35_000L) }
    } else null
    try { Mobile.request(peerId, path, body) }
    finally { if (wakeLock?.isHeld == true) wakeLock.release() }
  }

  @ReactMethod
  fun stop(promise: Promise) = submit(control, promise) {
    Mobile.stop()
    null
  }

  @ReactMethod
  fun logout(promise: Promise) = submit(control, promise) {
    Mobile.logout()
    null
  }

  override fun invalidate() {
    invalidated = true
    try { networkProvider.unregister() } catch (_: IllegalArgumentException) { /* Already unregistered. */ }
    // Never close tsnet synchronously on the UI/native-module thread.
    Thread({
      try { Mobile.stop() } catch (_: Exception) { /* Bridge is already closed. */ }
    }, "ShellfoxNetworkStop").apply { isDaemon = true }.start()
    control.shutdown()
    requests.shutdown()
    polls.shutdown()
    super.invalidate()
  }
}
