package com.shellfox.mobile

import android.content.ContentProvider
import android.content.ContentValues
import android.database.Cursor
import android.net.Uri
import android.webkit.WebView

/** CDP access belongs only to the isolated terminalTest APK, never preview or release. */
class TerminalTestWebView : ContentProvider() {
  override fun onCreate(): Boolean {
    WebView.setWebContentsDebuggingEnabled(true)
    return true
  }

  override fun query(uri: Uri, projection: Array<out String>?, selection: String?,
      selectionArgs: Array<out String>?, sortOrder: String?): Cursor? = null
  override fun getType(uri: Uri): String? = null
  override fun insert(uri: Uri, values: ContentValues?): Uri? = null
  override fun delete(uri: Uri, selection: String?, selectionArgs: Array<out String>?): Int = 0
  override fun update(uri: Uri, values: ContentValues?, selection: String?,
      selectionArgs: Array<out String>?): Int = 0
}
