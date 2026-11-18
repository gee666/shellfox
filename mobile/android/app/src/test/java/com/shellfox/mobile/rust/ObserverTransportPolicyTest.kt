package com.shellfox.mobile.rust

import com.shellfox.mobile.MobileEndpoint
import org.junit.Assert.*
import org.junit.Test

class ObserverTransportPolicyTest {
  @Test fun exactTenPathsSharePollAndFiniteBackgroundClassifiers() {
    val paths = setOf("/v1/discover", "/v1/pair", "/v1/request", "/v1/events", "/v1/disconnect",
      "/v2/discover", "/v2/pair", "/v2/request", "/v2/events", "/v2/disconnect")
    assertEquals(paths, MobileEndpoint.entries.map { it.path }.toSet())
    assertEquals(setOf("/v1/events", "/v2/events"), MobileEndpoint.entries.filter { it.isPoll }.map { it.path }.toSet())
    assertEquals(setOf("/v1/pair", "/v2/pair", "/v1/disconnect", "/v2/disconnect"),
      MobileEndpoint.entries.filter { it.needsBackgroundCompletion }.map { it.path }.toSet())
    assertEquals(30000, MobileEndpoint.DEADLINE_MS)
    for (bad in listOf("/v2/events/", "/v2/events?x=1", "/V2/events", "https://host/v2/discover", "/v2/stop"))
      assertNull(MobileEndpoint.fromPath(bad))
  }
  @Test fun actualHttpStatusesPassWithoutAuthenticationOrLegacyCoercion() {
    for (status in listOf(302, 401, 403, 404, 500, 599)) {
      val reply = NativeHttpReply.received(status.toLong(), "", 1024)
      assertEquals(status, reply.status)
      assertTrue(reply.bytes.isEmpty())
    }
    val business = """{"version":2,"ok":false,"error":{"code":"UNSUPPORTED"}}"""
    val success = NativeHttpReply.received(200, business, 1024)
    assertEquals(200, success.status)
    assertArrayEquals(business.toByteArray(), success.bytes)
  }
  @Test fun invalidResponsesNeverBecomeSynthetic404OrSuccess() {
    for (status in listOf(0L, 99L, 600L, Long.MAX_VALUE))
      assertThrows(NativeInvalidHttpReply::class.java) { NativeHttpReply.received(status, "", 1024) }
    assertThrows(NativeInvalidHttpReply::class.java) { NativeHttpReply.received(404, "diagnostic", 1024) }
    assertThrows(NativeResponseTooLarge::class.java) { NativeHttpReply.received(200, "large", 1) }
  }
  @Test fun profileCapabilitiesAreStaticApisAndNeverInventV1Authority() {
    val v1 = ClientProfile.decode(intArrayOf(1, 7))
    assertEquals(ClientProfile.V1, v1)
    assertTrue(v1.allowsInputApi && v1.allowsHostLayoutApi)
    val observer = ClientProfile.decode(intArrayOf(2, 1))
    assertTrue(observer.fixedCanonicalGrid)
    assertFalse(observer.allowsInputApi || observer.allowsHostLayoutApi)
    for (bad in listOf(intArrayOf(2, 7), intArrayOf(1, 1), intArrayOf(3, 1), intArrayOf(2)))
      assertThrows(IllegalArgumentException::class.java) { ClientProfile.decode(bad) }
  }
}
