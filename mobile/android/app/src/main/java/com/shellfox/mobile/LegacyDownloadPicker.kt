package com.shellfox.mobile

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.provider.DocumentsContract
import com.facebook.react.bridge.*

/** Android 7-9 export without requesting broad external-storage permission. */
internal class LegacyDownloadPicker(private val context: ReactApplicationContext) : BaseActivityEventListener() {
  private data class Choice(val token: String, val promise: Promise, val receive: (Uri) -> Unit)
  private var choice: Choice? = null
  init { context.addActivityEventListener(this) }
  fun choose(token: String, name: String, mime: String, promise: Promise, receive: (Uri) -> Unit) {
    UiThreadUtil.runOnUiThread {
      val activity = context.currentActivity
      if (choice != null || activity == null) { promise.reject("DOWNLOAD_PICKER", "The save picker is unavailable."); return@runOnUiThread }
      choice = Choice(token, promise, receive)
      try {
        activity.startActivityForResult(Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
          addCategory(Intent.CATEGORY_OPENABLE); type = mime
          putExtra(Intent.EXTRA_TITLE, name)
          addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
        }, REQUEST)
      } catch (error: Exception) { choice = null; promise.reject("DOWNLOAD_PICKER", error.message, error) }
    }
  }
  override fun onActivityResult(activity: Activity, requestCode: Int, resultCode: Int, data: Intent?) {
    if (requestCode != REQUEST) return
    val current = choice ?: return
    choice = null
    val uri = data?.data
    if (resultCode != Activity.RESULT_OK || uri == null) { current.promise.resolve(null); return }
    try {
      context.contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
      current.receive(uri)
    } catch (error: Exception) {
      try { DocumentsContract.deleteDocument(context.contentResolver, uri) } catch (_: Exception) { }
      current.promise.reject("DOWNLOAD_PICKER", error.message, error)
    }
  }
  fun cancel(token: String? = null) {
    UiThreadUtil.runOnUiThread {
      val current = choice
      if (current != null && (token == null || current.token == token)) {
        choice = null; context.currentActivity?.finishActivity(REQUEST); current.promise.resolve(null)
      }
    }
  }
  fun dispose() { cancel(); context.removeActivityEventListener(this) }
  companion object { private const val REQUEST = 6127 }
}
