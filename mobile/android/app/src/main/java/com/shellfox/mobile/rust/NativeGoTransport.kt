package com.shellfox.mobile.rust

import androidx.annotation.Keep
import android.content.Context
import com.shellfox.mobile.withPairApprovalWakeLock
import com.shellfox.mobile.MobileEndpoint

import mobile.Mobile
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Future
import java.util.concurrent.ScheduledThreadPoolExecutor
import java.util.concurrent.SynchronousQueue
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.atomic.AtomicBoolean

/** Uses the same outbound-only gomobile singleton as ShellfoxNetworkModule. */
@Keep
internal object NativeGoTransport {
  @Volatile private var application: Context? = null
  fun initialize(context: Context) { application = context.applicationContext }
  private data class Token(val client: Long, val generation: Long, val request: Long)
  private class Call {
    val finished = AtomicBoolean()
    @Volatile var work: Future<*>? = null
    @Volatile var deadline: Future<*>? = null
  }
  private val calls = ConcurrentHashMap<Token, Call>()
  private val deadlines = ScheduledThreadPoolExecutor(1) { task ->
    Thread(task, "ShellfoxRustDeadline").apply { isDaemon = true }
  }.apply { removeOnCancelPolicy = true }
  private fun pool(size: Int, name: String) = ThreadPoolExecutor(
    0, size, 60, TimeUnit.SECONDS, SynchronousQueue(),
    { task -> Thread(task, name).apply { isDaemon = true } }, ThreadPoolExecutor.AbortPolicy(),
  )
  private val requests = pool(12, "ShellfoxRustRequest")
  private val polls = pool(64, "ShellfoxRustPoll")

  @JvmStatic fun request(client: Long, generation: Long, request: Long, peer: ByteArray,
    path: ByteArray, body: ByteArray, deadlineMs: Int, maxResponse: Long) {
    val token = Token(client, generation, request)
    val call = Call()
    if (calls.putIfAbsent(token, call) != null) return
    fun finish(outcome: Int, bytes: ByteArray = byteArrayOf()) {
      if (!call.finished.compareAndSet(false, true)) return
      calls.remove(token, call)
      call.deadline?.cancel(false)
      // Rust fences reconnect/close generations too. A late completion cannot revive a client.
      ClientBindings.complete(client, generation, request, outcome, bytes)
    }
    val route = path.toString(Charsets.UTF_8)
    val endpoint = MobileEndpoint.fromPath(route)
    if (endpoint == null) { finish(6); return }
    if (deadlineMs !in 1..MobileEndpoint.DEADLINE_MS || maxResponse !in 1..8_388_608 || body.size > 1_048_576) {
      finish(2); return
    }
    call.deadline = deadlines.schedule({ finish(2); call.work?.cancel(true) }, deadlineMs.toLong(), TimeUnit.MILLISECONDS)
    try {
      call.work = (if (endpoint.isPoll) polls else requests).submit {
        if (call.finished.get()) return@submit
        try {
          val result = withPairApprovalWakeLock(application, route, deadlineMs.toLong()) {
            Mobile.request(peer.toString(Charsets.UTF_8), route, body.toString(Charsets.UTF_8))
          }.toByteArray(Charsets.UTF_8)
          if (result.size > maxResponse) finish(4) else finish(0, result)
        } catch (_: Exception) { finish(3) }
      }
      if (call.finished.get()) call.work?.cancel(true)
    } catch (_: RejectedExecutionException) { finish(3) }
  }

  @JvmStatic fun cancel(client: Long, generation: Long, request: Long) {
    val token = Token(client, generation, request)
    val call = calls.remove(token) ?: return
    if (!call.finished.compareAndSet(false, true)) return
    call.deadline?.cancel(false)
    // gomobile Request has no per-request cancel API. Interrupt and discard its
    // result; the occupied worker remains bounded until Go's own deadline ends.
    call.work?.cancel(true)
  }
}
