package com.shellfox.mobile

/** Exact Go carrier paths. This classification never negotiates a client profile. */
internal enum class MobileEndpoint(val path: String, private val traffic: Traffic) {
  V1_DISCOVER("/v1/discover", Traffic.REQUEST),
  V1_PAIR("/v1/pair", Traffic.PAIR),
  V1_REQUEST("/v1/request", Traffic.REQUEST),
  V1_EVENTS("/v1/events", Traffic.POLL),
  V1_DISCONNECT("/v1/disconnect", Traffic.DISCONNECT);

  private enum class Traffic { REQUEST, PAIR, POLL, DISCONNECT }
  val isPoll: Boolean get() = traffic == Traffic.POLL
  val needsBackgroundCompletion: Boolean get() = traffic == Traffic.PAIR || traffic == Traffic.DISCONNECT

  companion object {
    const val DEADLINE_MS = 30000
    fun fromPath(path: String): MobileEndpoint? = entries.firstOrNull { it.path == path }
  }
}
