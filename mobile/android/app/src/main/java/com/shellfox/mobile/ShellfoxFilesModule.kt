package com.shellfox.mobile

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.CancellationSignal
import android.os.OperationCanceledException
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.util.Base64
import com.facebook.react.bridge.*
import java.io.Closeable
import java.io.File
import java.io.RandomAccessFile
import java.util.UUID
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.SynchronousQueue
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference

/** Only picker-owned cache handles cross the bridge. Provider IO never runs on the UI thread. */
class ShellfoxFilesModule(context: ReactApplicationContext) : ReactContextBaseJavaModule(context), ActivityEventListener, LifecycleEventListener {
  private class Selection(val token: String, val promise: Promise, val requestCode: Int) {
    val cancelled = AtomicBoolean(false)
    val signal = CancellationSignal()
    val resource = AtomicReference<Closeable?>(null)
    val grantHeld = AtomicBoolean(false)
    @Volatile var uri: Uri? = null
    @Volatile var file: File? = null
  }
  // Local cache reads/releases must not queue behind an unresponsive cloud provider.
  private val worker = Executors.newSingleThreadExecutor()
  private val providers = ThreadPoolExecutor(2, 2, 15, TimeUnit.SECONDS, SynchronousQueue<Runnable>()).apply { allowCoreThreadTimeOut(true) }
  // Signal cancellation and descriptor close are independent: either can block in a provider.
  private val cancellations = ThreadPoolExecutor(4, 4, 15, TimeUnit.SECONDS, ArrayBlockingQueue<Runnable>(8)).apply { allowCoreThreadTimeOut(true) }
  private val handles = mutableMapOf<String, File>()
  private val copies = mutableSetOf<Selection>()
  private val lock = Any()
  private var selection: Selection? = null
  private val nextRequest = AtomicInteger(6183)
  private val closed = AtomicBoolean(false)
  private val cache = File(context.cacheDir, "terminal-attachments")
  private val grantLock = Any()
  private val grantUsers = mutableMapOf<String, Int>()
  private val preferences = context.getSharedPreferences("terminal-attachment-grants", Context.MODE_PRIVATE)

  init {
    context.addActivityEventListener(this)
    context.addLifecycleEventListener(this)
    worker.execute {
      cache.deleteRecursively(); cache.mkdirs()
      // Recover only this module's grants after a process dies during a provider copy.
      preferences.getStringSet("uris", emptySet())?.toList()?.forEach { releaseOwnedGrant(Uri.parse(it)) }
    }
  }
  override fun getName() = "ShellfoxFiles"
  private fun submit(onClosed: () -> Unit = {}, task: () -> Unit) {
    try { worker.execute { task() } } catch (_: RejectedExecutionException) { onClosed() }
  }
  private fun cancelTask(task: () -> Unit) {
    try { cancellations.execute { try { task() } catch (_: Exception) { } } }
    catch (_: RejectedExecutionException) { /* Teardown closes registration before shutting down this pool. */ }
  }
  private fun register(current: Selection, resource: Closeable) {
    current.resource.set(resource)
    if (current.cancelled.get() || closed.get()) {
      current.resource.getAndSet(null)?.let { cancelTask { it.close() } }
      throw OperationCanceledException()
    }
  }
  private fun cancel(current: Selection) {
    if (!current.cancelled.compareAndSet(false, true)) return
    if (synchronized(lock) { copies.contains(current) }) cancelTask { current.signal.cancel() }
    current.resource.getAndSet(null)?.let { cancelTask { it.close() } }
    // Cache deletion and grant release do not depend on the blocked provider returning.
    submit { current.file?.delete(); releaseGrant(current) }
  }
  private fun acquireGrant(current: Selection, uri: Uri) {
    synchronized(grantLock) {
      if (current.cancelled.get()) throw OperationCanceledException()
      val key = uri.toString()
      val users = grantUsers[key] ?: 0
      if (users > 0) { grantUsers[key] = users + 1; current.grantHeld.set(true); return }
      val resolver = reactApplicationContext.contentResolver
      // Never take ownership of a grant held by another feature.
      if (resolver.persistedUriPermissions.any { it.uri == uri && it.isReadPermission }) return
      val owned = preferences.getStringSet("uris", emptySet()).orEmpty().toMutableSet()
      owned.add(key)
      if (!preferences.edit().putStringSet("uris", owned).commit()) throw IllegalStateException("Could not record file access.")
      try {
        resolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
        grantUsers[key] = 1; current.grantHeld.set(true)
      } catch (_: SecurityException) { owned.remove(key); preferences.edit().putStringSet("uris", owned).commit() }
    }
  }
  private fun releaseGrant(current: Selection) {
    synchronized(grantLock) {
      if (!current.grantHeld.compareAndSet(true, false)) return
      val uri = current.uri ?: return
      val users = grantUsers[uri.toString()] ?: 0
      if (users > 1) { grantUsers[uri.toString()] = users - 1; return }
      grantUsers.remove(uri.toString())
      releaseOwnedGrant(uri)
    }
  }
  private fun releaseOwnedGrant(uri: Uri) {
    synchronized(grantLock) {
      val owned = preferences.getStringSet("uris", emptySet()).orEmpty().toMutableSet()
      if (!owned.contains(uri.toString())) return
      try { reactApplicationContext.contentResolver.releasePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION) }
      catch (_: SecurityException) { /* A failed take or expired grant needs only journal cleanup. */ }
      owned.remove(uri.toString()); preferences.edit().putStringSet("uris", owned).commit()
    }
  }

  @ReactMethod
  fun pick(token: String, promise: Promise) {
    if (!validId(token)) { promise.reject("VALIDATION", "Invalid picker token."); return }
    UiThreadUtil.runOnUiThread {
      val activity = reactApplicationContext.currentActivity
      if (activity == null) { promise.reject("PICKER_UNAVAILABLE", "The file picker is unavailable."); return@runOnUiThread }
      val next = Selection(token, promise, nextRequest.getAndUpdate { if (it >= 32760) 6183 else it + 1 })
      synchronized(lock) {
        if (closed.get() || selection != null || handles.size >= 4) {
          promise.reject("PICKER_UNAVAILABLE", "The file picker is unavailable or already open."); return@runOnUiThread
        }
        selection = next
      }
      try {
        // Do not request CATEGORY_OPENABLE: virtual documents can export a typed stream.
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
          type = "*/*"
          addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
        }
        activity.startActivityForResult(intent, next.requestCode)
      } catch (error: Exception) { complete(next, null, error) }
    }
  }
  override fun onActivityResult(activity: Activity, requestCode: Int, resultCode: Int, data: Intent?) {
    val current = synchronized(lock) { selection } ?: return
    if (requestCode != current.requestCode) return
    val uri = data?.data
    if (resultCode != Activity.RESULT_OK || uri == null) { complete(current, null, null); return }
    current.uri = uri
    current.file = File(cache, UUID.randomUUID().toString())
    submit(onClosed = { complete(current, null, IllegalStateException("The file module was closed.")) }) {
      synchronized(lock) { if (current.cancelled.get() || closed.get()) return@submit; copies.add(current) }
      try { providers.execute { copy(current, uri, data?.flags ?: 0) } }
      catch (_: RejectedExecutionException) {
        copyEnded(current)
        complete(current, null, IllegalStateException("The file provider is still cancelling. Try again."))
      }
    }
  }
  private fun copy(current: Selection, uri: Uri, flags: Int) {
    val file = current.file ?: return
    try {
      if (current.cancelled.get() || closed.get()) throw OperationCanceledException()
      current.signal.throwIfCanceled()
      val resolver = reactApplicationContext.contentResolver
      if ((flags and Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION) != 0) acquireGrant(current, uri)
      if (current.cancelled.get() || closed.get()) throw OperationCanceledException()
      var name = "attachment"
      var virtual = false
      val columns = if (DocumentsContract.isDocumentUri(reactApplicationContext, uri))
        arrayOf(OpenableColumns.DISPLAY_NAME, DocumentsContract.Document.COLUMN_FLAGS) else arrayOf(OpenableColumns.DISPLAY_NAME)
      resolver.query(uri, columns, null, null, null, current.signal)?.use { cursor ->
        if (cursor.moveToFirst()) {
          if (!cursor.isNull(0)) name = cursor.getString(0)
          virtual = columns.size > 1 && (cursor.getInt(1) and DocumentsContract.Document.FLAG_VIRTUAL_DOCUMENT) != 0
        }
      }
      name = name.replace(Regex("[\\x00-\\x1f\\x7f]"), "_").take(1024).ifEmpty { "attachment" }
      current.signal.throwIfCanceled()
      val descriptor = if (virtual) {
        val mime = resolver.getStreamTypes(uri, "*/*")?.firstOrNull() ?: throw IllegalStateException("This provider cannot export the selected file.")
        resolver.openTypedAssetFileDescriptor(uri, mime, null, current.signal)
      } else resolver.openAssetFileDescriptor(uri, "r", current.signal)
      if (descriptor == null) throw IllegalStateException("Could not open the selected file.")
      register(current, descriptor)
      val source = descriptor.createInputStream()
      register(current, source)
      var size = 0L
      source.use { input ->
        file.outputStream().use { destination ->
          val buffer = ByteArray(64 * 1024)
          while (true) {
            current.signal.throwIfCanceled()
            if (current.cancelled.get() || closed.get()) throw OperationCanceledException()
            val count = input.read(buffer)
            if (count < 0) break
            size += count
            if (size > MAX_FILE) throw IllegalStateException("Choose a file no larger than 64 MiB.")
            destination.write(buffer, 0, count)
          }
        }
      }
      current.resource.set(null)
      val handle = UUID.randomUUID().toString()
      synchronized(lock) {
        if (current.cancelled.get() || closed.get() || selection !== current) return
        handles[handle] = file
        complete(current, Arguments.createMap().apply { putString("handle", handle); putString("fileName", name); putDouble("size", size.toDouble()) }, null)
      }
    } catch (error: Exception) { complete(current, null, error) }
    finally {
      current.resource.getAndSet(null)?.let { cancelTask { it.close() } }
      if (synchronized(lock) { !handles.containsValue(file) }) file.delete()
      submit { releaseGrant(current) }
      copyEnded(current)
    }
  }
  private fun copyEnded(current: Selection) {
    synchronized(lock) {
      copies.remove(current)
      if (closed.get() && copies.isEmpty()) cancellations.shutdown()
    }
  }
  private fun complete(current: Selection, value: WritableMap?, error: Exception?) {
    synchronized(lock) {
      if (selection !== current) return
      selection = null
      if (error != null) current.promise.reject("FILE_PICK", error.message ?: "Could not read the selected file.", error)
      else current.promise.resolve(value)
    }
  }
  @ReactMethod
  fun cancelPick(token: String, promise: Promise) {
    if (!validId(token)) { promise.reject("VALIDATION", "Invalid picker token."); return }
    UiThreadUtil.runOnUiThread {
      synchronized(lock) {
        val current = selection
        if (current?.token == token) {
          cancel(current); complete(current, null, null)
          reactApplicationContext.currentActivity?.finishActivity(current.requestCode)
        }
      }
      promise.resolve(null)
    }
  }
  @ReactMethod
  fun read(handle: String, offset: Double, length: Double, promise: Promise) {
    if (!validId(handle) || !offset.isFinite() || offset < 0 || offset > MAX_FILE || offset % 1 != 0.0 || !length.isFinite() || length % 1 != 0.0 || length < 1 || length > MAX_CHUNK) {
      promise.reject("VALIDATION", "Invalid file chunk."); return
    }
    if (closed.get()) { promise.reject("FILE_READ", "The file module was closed."); return }
    submit(onClosed = { promise.reject("FILE_READ", "The file module was closed.") }) {
      try {
        val file = synchronized(lock) { handles[handle] } ?: throw IllegalStateException("The selected file was released.")
        if (closed.get() || offset.toLong() + length.toLong() > file.length()) throw IllegalStateException("The selected file changed.")
        val bytes = ByteArray(length.toInt())
        RandomAccessFile(file, "r").use { input -> input.seek(offset.toLong()); input.readFully(bytes) }
        promise.resolve(Arguments.createMap().apply { putString("data", Base64.encodeToString(bytes, Base64.NO_WRAP)); putInt("bytes", bytes.size) })
      } catch (error: Exception) { promise.reject("FILE_READ", error.message, error) }
    }
  }
  @ReactMethod
  fun release(handle: String, promise: Promise) {
    if (!validId(handle)) { promise.reject("VALIDATION", "Invalid file handle."); return }
    if (closed.get()) { promise.resolve(null); return }
    submit(onClosed = { promise.resolve(null) }) {
      val file = synchronized(lock) { handles.remove(handle) }
      if (file != null && file.exists() && !file.delete()) promise.reject("FILE_RELEASE", "Could not remove the selected file cache.") else promise.resolve(null)
    }
  }
  private fun cleanup(final: Boolean) {
    if (final && !closed.compareAndSet(false, true) || !final && closed.get()) return
    synchronized(lock) {
      (copies + listOfNotNull(selection)).forEach { cancel(it) }
      selection?.let { complete(it, null, null) }
      if (final && copies.isEmpty()) cancellations.shutdown()
    }
    submit {
      synchronized(lock) { handles.clear() }
      cache.deleteRecursively()
      if (!final) cache.mkdirs()
    }
    if (final) { providers.shutdown(); worker.shutdown() }
  }
  override fun invalidate() { cleanup(true); reactApplicationContext.removeActivityEventListener(this); reactApplicationContext.removeLifecycleEventListener(this); super.invalidate() }
  override fun onHostDestroy() { cleanup(false) }
  override fun onHostResume() = Unit
  override fun onHostPause() = Unit
  override fun onNewIntent(intent: Intent) = Unit
  companion object {
    private const val MAX_FILE = 64L * 1024 * 1024
    private const val MAX_CHUNK = 192 * 1024
    private fun validId(value: String) = value.matches(Regex("[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}"))
  }
}
