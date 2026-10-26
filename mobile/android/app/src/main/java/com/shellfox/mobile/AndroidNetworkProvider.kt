package com.shellfox.mobile

import android.content.Context
import android.net.ConnectivityManager
import android.net.LinkProperties
import android.net.Network
import mobile.Mobile
import mobile.NetworkProvider
import org.json.JSONArray
import org.json.JSONObject
import java.net.NetworkInterface
import java.util.Collections

/** Android's SDK can enumerate interfaces where Go's netlink cannot. */
class AndroidNetworkProvider(context: Context) : NetworkProvider {
  private val connectivity = context.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
  private val callback = object : ConnectivityManager.NetworkCallback() {
    override fun onAvailable(network: Network) { Mobile.networkChanged() }
    override fun onLost(network: Network) { Mobile.networkChanged() }
    override fun onLinkPropertiesChanged(network: Network, properties: LinkProperties) { Mobile.networkChanged() }
  }

  fun register() { connectivity.registerDefaultNetworkCallback(callback) }
  fun unregister() { connectivity.unregisterNetworkCallback(callback) }

  override fun interfaces(): String {
    val interfaces = JSONArray()
    val enumeration = NetworkInterface.getNetworkInterfaces()
    for (network in if (enumeration == null) emptyList() else Collections.list(enumeration)) {
      val addresses = JSONArray()
      for (address in network.interfaceAddresses) {
        // Go prefixes do not carry Android's IPv6 interface scope suffix.
        val ip = address.address.hostAddress?.substringBefore('%') ?: continue
        addresses.put("$ip/${address.networkPrefixLength}")
      }
      interfaces.put(JSONObject()
        .put("name", network.name)
        .put("index", network.index)
        .put("mtu", network.mtu.coerceAtLeast(0))
        .put("up", network.isUp)
        .put("loopback", network.isLoopback)
        .put("pointToPoint", network.isPointToPoint)
        .put("multicast", network.supportsMulticast())
        .put("addresses", addresses))
    }
    val properties = connectivity.activeNetwork?.let { connectivity.getLinkProperties(it) }
    val gateway = properties?.routes?.firstOrNull { it.isDefaultRoute && it.hasGateway() }?.gateway
    return JSONObject()
      .put("interfaces", interfaces)
      .put("defaultInterface", properties?.interfaceName ?: "")
      .put("gateway", gateway?.hostAddress?.substringBefore('%') ?: "")
      .toString()
  }
}
