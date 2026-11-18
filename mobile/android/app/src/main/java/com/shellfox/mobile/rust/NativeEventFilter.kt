package com.shellfox.mobile.rust

import org.json.JSONObject

/** Keep explicit Promise results; internal traffic only reports changed state/errors. */
internal class NativeEventFilter {
  private val explicit = mutableSetOf<String>()
  private var lastConnection = ""
  private var lastInternalError = ""
  fun expect(id: String) { explicit.add(id) }
  fun filter(event: JSONObject): String? {
    val connection = event.optJSONObject("connection")?.toString().orEmpty()
    val requested = explicit.remove(event.getString("operationId"))
    if (requested || !event.getBoolean("internal")) {
      // Explicit poll uses the Rust native-internal path but still completes its JS operation.
      event.put("internal", false)
      lastConnection = connection
      return event.toString()
    }
    val result = event.optJSONObject("result")
    val error = event.getInt("status") != 0 || result?.optBoolean("ok", true) == false
    val changes = result?.optJSONObject("value")?.optJSONArray("events")?.length() ?: 0
    val signature = "${event.getInt("status")}|$connection|${result?.optJSONObject("error")}"
    val changed = connection != lastConnection
    lastConnection = connection
    if (changes == 0 && !changed && (!error || signature == lastInternalError)) return null
    if (error) lastInternalError = signature else lastInternalError = ""
    return event.toString()
  }
}
