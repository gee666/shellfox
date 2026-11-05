package com.shellfox.mobile

import android.content.ContentValues
import android.net.Uri
import android.os.Environment
import android.os.Build
import android.os.SystemClock
import java.util.concurrent.TimeUnit
import android.provider.DocumentsContract
import android.content.Intent
import android.provider.MediaStore
import android.util.Base64
import com.facebook.react.bridge.*
import java.io.OutputStream
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.atomic.AtomicBoolean

/** Streaming, app-owned pending Downloads rows. No filesystem path crosses the bridge. */
class ShellfoxDownloadsModule(context: ReactApplicationContext) : ReactContextBaseJavaModule(context), LifecycleEventListener {
  private data class Download(val uri: Uri, val stream: OutputStream, val size: Long, val name: String, val pending: Boolean, var offset: Long = 0, var touched: Long = SystemClock.elapsedRealtime())
  private val worker = Executors.newSingleThreadScheduledExecutor()
  private val closed = AtomicBoolean(false)
  private val downloads = mutableMapOf<String, Download>()
  private val journal = context.getSharedPreferences("pending-downloads", 0)
  private val legacy = LegacyDownloadPicker(context)
  private val cancelled = mutableSetOf<String>()
  private val resolver get() = reactApplicationContext.contentResolver
  init {
    context.addLifecycleEventListener(this)
    worker.execute { journal.getStringSet("uris", emptySet()).orEmpty().toList().forEach { discard(Uri.parse(it)) } }
    worker.scheduleAtFixedRate({
      val stale = downloads.filterValues { SystemClock.elapsedRealtime() - it.touched > 300_000 }.keys.toList()
      stale.forEach { id -> downloads.remove(id)?.let { current -> try { current.stream.close() } catch (_: Exception) { }; discard(current.uri) } }
    }, 60, 60, TimeUnit.SECONDS)
  }
  override fun getName() = "ShellfoxDownloads"
  private fun task(promise: Promise, work: () -> Unit) {
    try { worker.execute {
      if (closed.get()) { promise.reject("DOWNLOAD_CLOSED", "Downloads are unavailable."); return@execute }
      try { work() } catch (error: Exception) { promise.reject("DOWNLOAD_IO", error.message ?: "Could not save the download.", error) }
    } } catch (_: RejectedExecutionException) { promise.reject("DOWNLOAD_CLOSED", "Downloads are unavailable.") }
  }
  private fun remember(uri: Uri, add: Boolean) {
    val values = journal.getStringSet("uris", emptySet()).orEmpty().toMutableSet()
    if (add) values.add(uri.toString()) else values.remove(uri.toString())
    if (!journal.edit().putStringSet("uris", values).commit()) throw IllegalStateException("Could not record pending download.")
  }
  private fun discard(uri: Uri) {
    try {
      // A finish may publish a row just before process death. Never delete a published file.
      if (uri.authority == "media") resolver.delete(uri, "${MediaStore.MediaColumns.IS_PENDING}=1", null)
      else { DocumentsContract.deleteDocument(resolver, uri); releaseGrant(uri) }
      remember(uri, false)
    } catch (_: Exception) { /* Keep the journal entry for the next startup. */ }
  }
  private fun releaseGrant(uri: Uri) {
    try { resolver.releasePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION) } catch (_: SecurityException) { }
  }
  private fun validId(value: String) = value.matches(Regex("[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}"))
  @ReactMethod
  fun begin(token: String, fileName: String, size: Double, mimeType: String, promise: Promise) {
    if (!validId(token) || fileName.isEmpty() || fileName.length > 255 || fileName == "." || fileName == ".." || fileName.contains(Regex("[\\x00-\\x1f\\x7f/\\\\]")) ||
      !size.isFinite() || size < 0 || size > MAX_FILE || size % 1 != 0.0 || !mimeType.matches(Regex("[A-Za-z0-9.+-]+/[A-Za-z0-9.+-]+"))) {
      promise.reject("VALIDATION", "Invalid download name, size or type."); return
    }
    if (Build.VERSION.SDK_INT < 29) {
      legacy.choose(token, fileName, mimeType, promise) { uri -> task(promise) { open(token, uri, size, fileName, false, promise) } }
      return
    }
    task(promise) {
      if (downloads.size >= 4 || downloads.containsKey(token)) throw IllegalStateException("Finish another download first.")
      val values = ContentValues().apply {
        put(MediaStore.MediaColumns.DISPLAY_NAME, fileName)
        put(MediaStore.MediaColumns.MIME_TYPE, mimeType)
        put(MediaStore.MediaColumns.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS)
        put(MediaStore.MediaColumns.IS_PENDING, 1)
      }
      val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values) ?: throw IllegalStateException("Could not create the download.")
      open(token, uri, size, fileName, true, promise)
    }
  }
  private fun open(token: String, uri: Uri, size: Double, name: String, pending: Boolean, promise: Promise) {
    try {
      remember(uri, true)
      if (cancelled.remove(token) || downloads.size >= 4 || downloads.containsKey(token)) throw IllegalStateException("The download was cancelled or is already open.")
      val stream = resolver.openOutputStream(uri, "w") ?: throw IllegalStateException("Could not open the download.")
      val actualName = try { resolver.query(uri, arrayOf(MediaStore.MediaColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
        if (cursor.moveToFirst() && !cursor.isNull(0)) cursor.getString(0) else name
      } ?: name } catch (_: Exception) { name }
      downloads[token] = Download(uri, stream, size.toLong(), actualName, pending)
      promise.resolve(Arguments.createMap().apply { putString("handle", token) })
    } catch (error: Exception) { discard(uri); throw error }
  }
  @ReactMethod
  fun write(handle: String, offset: Double, data: String, promise: Promise) {
    if (!validId(handle) || !offset.isFinite() || offset < 0 || offset > MAX_FILE || offset % 1 != 0.0 || data.length > MAX_CHUNK * 4 / 3 ||
      !data.matches(Regex("[A-Za-z0-9+/]*={0,2}"))) {
      promise.reject("VALIDATION", "Invalid download chunk."); return
    }
    task(promise) {
      val current = downloads[handle] ?: throw IllegalStateException("The download was cancelled.")
      val bytes = Base64.decode(data, Base64.NO_WRAP)
      if (bytes.isEmpty() || bytes.size > MAX_CHUNK || Base64.encodeToString(bytes, Base64.NO_WRAP) != data || offset.toLong() != current.offset || current.offset + bytes.size > current.size) {
        throw IllegalStateException("Unexpected download offset or data.")
      }
      current.stream.write(bytes)
      current.offset += bytes.size; current.touched = SystemClock.elapsedRealtime()
      promise.resolve(Arguments.createMap().apply { putDouble("offset", current.offset.toDouble()) })
    }
  }
  @ReactMethod
  fun finish(handle: String, promise: Promise) {
    if (!validId(handle)) { promise.reject("VALIDATION", "Invalid download handle."); return }
    task(promise) {
      val current = downloads[handle] ?: throw IllegalStateException("The download was cancelled.")
      if (current.offset != current.size) throw IllegalStateException("The download is incomplete.")
      current.stream.flush(); current.stream.close()
      if (current.pending) {
        val published = ContentValues().apply { put(MediaStore.MediaColumns.IS_PENDING, 0) }
        if (resolver.update(current.uri, published, null, null) != 1) throw IllegalStateException("Could not publish the download.")
      } else { remember(current.uri, false); releaseGrant(current.uri) }
      downloads.remove(handle)
      if (current.pending) try { remember(current.uri, false) } catch (_: Exception) { /* Recovery preserves already-published rows. */ }
      promise.resolve(Arguments.createMap().apply { putString("fileName", current.name); putString("location", "Downloads") })
    }
  }
  @ReactMethod
  fun cancel(handle: String, promise: Promise) {
    if (!validId(handle)) { promise.reject("VALIDATION", "Invalid download handle."); return }
    legacy.cancel(handle)
    task(promise) {
      if (!downloads.containsKey(handle) && cancelled.size < 32) cancelled.add(handle)
      downloads.remove(handle)?.let { current -> try { current.stream.close() } finally { discard(current.uri) } }
      promise.resolve(null)
    }
  }
  private fun cleanup() {
    downloads.values.toList().forEach { current -> try { current.stream.close() } catch (_: Exception) { }; discard(current.uri) }
    downloads.clear()
  }
  override fun invalidate() {
    if (closed.compareAndSet(false, true)) { worker.execute { cleanup() }; worker.shutdown() }
    legacy.dispose()
    reactApplicationContext.removeLifecycleEventListener(this)
    super.invalidate()
  }
  override fun onHostDestroy() { legacy.cancel(); if (!closed.get()) try { worker.execute { cleanup() } } catch (_: RejectedExecutionException) { } }
  override fun onHostResume() = Unit
  override fun onHostPause() = Unit
  companion object { private const val MAX_FILE = 1024L * 1024 * 1024; private const val MAX_CHUNK = 192 * 1024 }
}
