package com.shellfox.mobile.rust

import android.net.IpPrefix
import android.net.RouteInfo
import com.shellfox.mobile.defaultGateway
import java.net.InetAddress
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.util.ReflectionHelpers
import org.robolectric.util.ReflectionHelpers.ClassParameter

@RunWith(RobolectricTestRunner::class)
@Config(manifest = Config.NONE, sdk = [24], application = android.app.Application::class)
class AndroidNetworkProviderTest {
  // RouteInfo instances are supplied by the OS; its constructors are hidden SDK APIs.
  private fun route(prefix: IpPrefix, gateway: InetAddress): RouteInfo = ReflectionHelpers.callConstructor(
    RouteInfo::class.java, ClassParameter.from(IpPrefix::class.java, prefix),
    ClassParameter.from(InetAddress::class.java, gateway), ClassParameter.from(String::class.java, "fixture0"),
  )
  @Test fun choosesDefaultGatewayWithoutApi29RouteMethods() {
    val address = InetAddress.getByAddress(byteArrayOf(192.toByte(), 0, 2, 1))
    val prefix = IpPrefix(InetAddress.getByAddress(ByteArray(4)), 0)
    val direct = route(prefix, InetAddress.getByAddress(ByteArray(4)))
    val specific = route(IpPrefix(address, 24), address)
    val default = route(prefix, address)
    assertNull(defaultGateway(listOf(direct, specific)))
    assertEquals(address, defaultGateway(listOf(direct, specific, default)))
  }
  @Test fun acceptsIpv6DefaultAndRejectsMissingGateway() {
    val address = InetAddress.getByAddress(ByteArray(16).apply { this[0] = 0x20; this[1] = 1; this[15] = 1 })
    val route = route(IpPrefix(InetAddress.getByAddress(ByteArray(16)), 0), address)
    assertEquals(address, defaultGateway(listOf(route)))
    assertNull(defaultGateway(emptyList()))
  }
}
