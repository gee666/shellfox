package com.shellfox.mobile.rust

/** Available native APIs, not current peer approval, ownership or layout grants. */
internal enum class ClientProfile(val code: Int, val wireName: String, val apiCapabilities: Int) {
  V1(1, "v1", 7),
  OBSERVER_V2(2, "observer-v2", 1),
  MANAGER_V2(3, "manager-v2", 7);

  val allowsInputApi: Boolean get() = apiCapabilities and 2 != 0
  val allowsHostLayoutApi: Boolean get() = apiCapabilities and 4 != 0
  val fixedCanonicalGrid: Boolean get() = this != V1
  val localParserResize: Boolean get() = this == V1

  companion object {
    fun decode(value: IntArray): ClientProfile {
      require(value.size == 2)
      return entries.singleOrNull { it.code == value[0] && it.apiCapabilities == value[1] }
        ?: throw IllegalArgumentException("Unknown native client profile")
    }
  }
}
