package com.shellfox.mobile.rust

import androidx.annotation.Keep
import org.json.JSONObject
import java.util.concurrent.ConcurrentLinkedQueue

/** Test-only native dispatch endpoint. It never calls Go, HTTP, login or a PTY. */
@Keep
internal object FixtureTransport {
  private lateinit var paired: ByteArray
  private lateinit var attached: ByteArray
  val writes = ConcurrentLinkedQueue<JSONObject>()
  fun initialize(pair: ByteArray, attachment: ByteArray) {
    paired = pair.copyOf(); attached = attachment.copyOf(); writes.clear()
  }
  @JvmStatic fun request(client: Long, generation: Long, request: Long, peer: ByteArray,
    path: ByteArray, body: ByteArray, deadlineMs: Int, maxResponse: Long) {
    require(peer.toString(Charsets.UTF_8) == "n-fake-peer" && deadlineMs > 0)
    val payload = JSONObject(body.toString(Charsets.UTF_8))
    val reply = when (path.toString(Charsets.UTF_8)) {
      "/v1/pair" -> paired
      "/v1/events" -> """{"ok":true,"value":{"events":[{"type":"terminal","event":{"type":"data","tabId":"20000000-0000-4000-8000-000000000001","generation":"30000000-0000-4000-8000-000000000001","controlEpoch":1,"sequence":2,"data":"\u001b[>31u\u001b[?1003h\u001b[?1006h"}}]}}""".toByteArray()
      "/v1/request" -> when (payload.getString("method")) {
        "attachTerminal" -> attached
        "writeTerminal" -> {
          writes.add(payload)
          """{"ok":true,"value":{"written":true}}""".toByteArray()
        }
        else -> error("Unexpected fixture method")
      }
      else -> error("Unexpected fixture endpoint")
    }
    require(reply.size <= maxResponse)
    check(ClientBindings.complete(client, generation, request, 0, reply) == 0)
  }
  @JvmStatic fun cancel(client: Long, generation: Long, request: Long) {
    require(client != 0L && generation != 0L && request != 0L)
  }
}
