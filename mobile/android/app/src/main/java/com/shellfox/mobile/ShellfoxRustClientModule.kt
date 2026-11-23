package com.shellfox.mobile

import com.facebook.react.bridge.LifecycleEventListener
import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.module.annotations.ReactModule
import com.shellfox.mobile.rust.ClientEventSink
import com.shellfox.mobile.rust.ClientTerminalBindings
import com.shellfox.mobile.rust.NativeCallException
import com.shellfox.mobile.rust.nativeCode
import com.shellfox.mobile.rust.NativeInputBusy
import com.shellfox.mobile.rust.NativeInputStale
import com.shellfox.mobile.rust.RustBindings
import com.shellfox.mobile.rust.NativeGoTransport
import com.shellfox.mobile.rust.TerminalCoordinator
import org.json.JSONException
import org.json.JSONObject

private val fileRequests = setOf("getHomeDirectory", "listFiles", "readFile", "writeFile", "writeImageFile", "readFileAsset",
  "beginFileRead", "readFileChunk", "beginFileWrite", "writeFileChunk", "finishFileWrite", "cancelFileTransfer")

private fun fileRequest(json: String): String? {
  if (json.length > 1_048_576) return null
  // Send the object used for classification so duplicate keys cannot turn a
  // file request into a command that changes terminal state.
  return try { JSONObject(json).takeIf { command ->
    val method = command.opt("method")
    method is String && method in fileRequests
  }?.toString() }
  catch (_: JSONException) { null }
}

@ReactModule(name = NativeShellfoxRustClientSpec.NAME)
class ShellfoxRustClientModule(context: ReactApplicationContext) : NativeShellfoxRustClientSpec(context), LifecycleEventListener,
  ClientEventSink {
  override fun clientEvent(clientHandle: String) { if (!stopped) emitOnClientEvent(clientHandle) }
  init { NativeGoTransport.initialize(context); context.addLifecycleEventListener(this) }
  override fun onHostResume() { TerminalCoordinator.foreground(this, true) }
  override fun onHostPause() { TerminalCoordinator.foreground(this, false) }
  override fun onHostDestroy() { TerminalCoordinator.foreground(this, false) }
  @Volatile private var stopped = false
  override fun abiVersion(): Double = try { RustBindings.abiVersion().toDouble() } catch (_: LinkageError) { 0.0 }

  override fun createTerminal(cols: Double, rows: Double, promise: Promise) {
    if (unavailable(promise)) return
    TerminalCoordinator.create(this, cols, rows) { value, error -> result(promise, value, error) }
  }
  override fun closeTerminal(terminalHandle: String, promise: Promise) {
    if (unavailable(promise)) return
    TerminalCoordinator.close(this, terminalHandle) { result(promise, null, it) }
  }
  override fun progress(terminalHandle: String, promise: Promise) {
    if (unavailable(promise)) return
    TerminalCoordinator.progress(this, terminalHandle) { value, error -> result(promise, value, error) }
  }
  override fun terminalInput(terminalHandle: String, json: String, promise: Promise) = run(promise) {
    TerminalCoordinator.typedInput(this, terminalHandle, json); null
  }
  override fun configureTerminal(terminalHandle: String, json: String, promise: Promise) = run(promise) {
    TerminalCoordinator.configure(this, terminalHandle, json); null
  }
  override fun createClient(peerId: String, clientId: String, promise: Promise) = run(promise) {
    ClientTerminalBindings.create(this, peerId, clientId)
  }
  override fun connect(clientHandle: String, promise: Promise) = run(promise) {
    ClientTerminalBindings.start(this, clientHandle, 0)
  }
  override fun reconnect(clientHandle: String, promise: Promise) = afterPoll(clientHandle, promise, allowV1Fence = true) {
    ClientTerminalBindings.start(this, clientHandle, 1)
  }
  override fun poll(clientHandle: String, promise: Promise) = run(promise) {
    ClientTerminalBindings.start(this, clientHandle, 2)
  }
  override fun operation(clientHandle: String, json: String, promise: Promise) {
    val file = fileRequest(json)
    afterPoll(clientHandle, promise, independentFile = file != null) {
      ClientTerminalBindings.start(this, clientHandle, 3, file ?: json)
    }
  }
  override fun cancel(clientHandle: String, operationId: String, promise: Promise) = run(promise) {
    ClientTerminalBindings.cancel(this, clientHandle, operationId); null
  }
  override fun bindTerminal(clientHandle: String, tabId: String, terminalHandle: String, promise: Promise) = admit(promise) { done ->
    ClientTerminalBindings.changeViewBinding(this, clientHandle, tabId, terminalHandle, done)
  }
  override fun unbindTerminal(clientHandle: String, terminalHandle: String, promise: Promise) = admit(promise) { done ->
    ClientTerminalBindings.changeViewBinding(this, clientHandle, null, terminalHandle, done)
  }
  override fun readEvent(clientHandle: String, promise: Promise) = run(promise) {
    ClientTerminalBindings.readEvent(this, clientHandle)
  }
  override fun closeClient(clientHandle: String, promise: Promise) {
    if (unavailable(promise)) return
    TerminalCoordinator.execute({ result(promise, null, "BUSY") }) {
      try { ClientTerminalBindings.close(this, clientHandle) { result(promise, null, it) } }
      catch (_: Exception) { result(promise, null, "ENGINE") }
      catch (_: LinkageError) { result(promise, null, "UNAVAILABLE") }
    }
  }

  private fun unavailable(promise: Promise): Boolean {
    if (stopped) promise.reject("RUST_STOPPED", "Native client is unavailable.")
    return stopped
  }
  private fun afterPoll(client: String, promise: Promise, allowV1Fence: Boolean = false,
    independentFile: Boolean = false, action: () -> String?) = admit(promise) { done ->
    ClientTerminalBindings.afterPoll(this, client, {
      if (stopped) throw NativeCallException(3)
      action()
    }, allowV1Fence, independentFile, done)
  }
  private fun admit(promise: Promise, enqueue: ((String?, Exception?) -> Unit) -> Unit) {
    if (unavailable(promise)) return
    TerminalCoordinator.execute({ result(promise, null, "BUSY") }) {
      try {
        enqueue { value, error ->
          val code = when (error) {
            null -> null
            is NativeCallException -> nativeCode(error.status)
            is NativeInputBusy -> "BUSY"
            is NativeInputStale -> "STALE"
            is IllegalArgumentException -> "INVALID"
            else -> "ENGINE"
          }
          result(promise, value, code)
        }
      } catch (error: NativeCallException) { result(promise, null, nativeCode(error.status)) }
      catch (_: Exception) { result(promise, null, "ENGINE") }
    }
  }
  private fun run(promise: Promise, action: () -> String?) {
    if (unavailable(promise)) return
    TerminalCoordinator.execute({ result(promise, null, "BUSY") }) {
      if (stopped) { result(promise, null, "STOPPED"); return@execute }
      try { promise.resolve(action()) }
      catch (_: NativeInputBusy) { result(promise, null, "BUSY") }
      catch (_: NativeInputStale) { result(promise, null, "STALE") }
      catch (error: NativeCallException) { result(promise, null, nativeCode(error.status)) }
      catch (_: IllegalArgumentException) { result(promise, null, "INVALID") }
      catch (_: Exception) { result(promise, null, "ENGINE") }
      catch (_: LinkageError) { result(promise, null, "UNAVAILABLE") }
    }
  }
  private fun result(promise: Promise, value: String?, error: String?) {
    if (error == null) promise.resolve(value)
    else promise.reject("RUST_$error", "Native client operation failed.")
  }
  override fun invalidate() {
    stopped = true
    reactApplicationContext.removeLifecycleEventListener(this)
    TerminalCoordinator.release(this)
    super.invalidate()
  }
}
