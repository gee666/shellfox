package com.shellfox.mobile.rust

/** Reporting a deadline/error never changes the native-close proof. */
internal class ClientCloseGate {
  var disconnectSucceeded = false
    private set
  private var attempted = false
  var closed = false
    private set
  val uncertainClose: Boolean get() = attempted && !closed

  fun disconnectCompleted(success: Boolean) { disconnectSucceeded = success }
  fun finish(pollDrained: Boolean, ownerDrained: Boolean, nativeClose: () -> Unit): Boolean {
    if (closed) return true
    if (!pollDrained || !ownerDrained || !disconnectSucceeded || attempted) return false
    attempted = true
    nativeClose() // Failure retains custody. Never retry an uncertain close.
    closed = true
    return true
  }
}

/** Module invalidation retains its owner entries until actual client removal. */
internal class RetainedModuleRelease(
  private val clientsClosed: () -> Boolean,
  private val ownerDrained: () -> Boolean,
  private val closeEngines: () -> Boolean,
) {
  private var finished = false
  private var attempted = false
  fun advance(): Boolean {
    if (finished) return true
    if (attempted || !clientsClosed() || !ownerDrained()) return false
    attempted = true
    finished = closeEngines()
    return finished
  }
}
