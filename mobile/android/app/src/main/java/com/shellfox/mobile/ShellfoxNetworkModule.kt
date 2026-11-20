package com.shellfox.mobile

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
  private val discovery = immediateExecutor(12, "ShellfoxDiscovery")
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
  fun startWithControlServer(hostname: String, controlURL: String, promise: Promise) = submit(control, promise, start = true) {
    val originBytes = controlURL.toByteArray(Charsets.UTF_8)
    require(originBytes.size <= 2048)
    // State isolation only, never a URL validator or authority stamp.
    val stateId = java.security.MessageDigest.getInstance("SHA-256").digest(originBytes)
      .take(16).joinToString("") { (it.toInt() and 255).toString(16).padStart(2, '0') }
    val directory = File(reactApplicationContext.noBackupFilesDir, "shellfox-tailscale-loopback-$stateId")
    Mobile.startWithControlServer(directory.absolutePath, hostname, controlURL)
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
  fun peers(promise: Promise) = submit(discovery, promise) { Mobile.peers() }

  @ReactMethod
  fun hosts(promise: Promise) = submit(discovery, promise) { Mobile.hosts() }

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
    discovery.shutdown()
    super.invalidate()
  }
}
