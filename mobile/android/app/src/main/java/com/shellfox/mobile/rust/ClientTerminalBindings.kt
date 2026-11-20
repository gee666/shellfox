package com.shellfox.mobile.rust

import org.json.JSONObject
import java.util.ArrayDeque

/** Only accessed on TerminalCoordinator's OS thread. Rust owns leases and tickets. */
internal object ClientTerminalBindings {
  private class Client(val handle: Long, val module: Any, val profile: ClientProfile) {
    val events = ArrayDeque<String>()
    var eventBytes = 0
    var failed = false
    val filter = NativeEventFilter()
    val poll = ClientPollLane({ android.os.SystemClock.elapsedRealtime() },
      { idString(ClientBindings.start(handle, 2, null)) })
    var foreground = true
    var closing: Closing? = null
  }
  private class Closing(val deadline: Long, val callbacks: MutableList<(String?) -> Unit>) {
    var operation: String? = null
    val gate = ClientCloseGate()
  }
  private class Binding(val client: Client, terminal: Long) {
    var size: Pair<Int, Int>? = null
    val input = NativeInputAdmission(
      { bytes, hint -> if (client.profile == ClientProfile.MANAGER_V2) ClientBindings.inputManager(client.handle, terminal, bytes, hint) else ClientBindings.write(client.handle, terminal, bytes, hint) },
      { cols, rows -> ClientBindings.resize(client.handle, terminal, cols, rows) },
    )
  }
  private val resetting = mutableSetOf<String>()
  private val recovering = mutableSetOf<String>()
  private val clients = mutableMapOf<String, Client>()
  private val terminals = mutableMapOf<String, Binding>()
  private val transport = NativeGoTransport::class.java

  fun create(module: Any, peer: String, clientId: String, requested: ClientProfile = ClientProfile.V1): String {
    require(peer.toByteArray().size <= 256 && clientId.length <= 64)
    val handle = when (requested) {
      ClientProfile.V1 -> ClientBindings.create(peer.toByteArray(), clientId.toByteArray())
      ClientProfile.OBSERVER_V2 -> ClientBindings.createObserver(peer.toByteArray(), clientId.toByteArray())
      ClientProfile.MANAGER_V2 -> { require(ClientBindings.managerAbiVersion() == 1); ClientBindings.createManager(peer.toByteArray(), clientId.toByteArray()) }
    }
    val actual = try {
      ClientProfile.decode(ClientBindings.profile(handle)).also { require(it == requested) }
    } catch (error: Exception) { ClientBindings.close(handle, transport); throw error }
    val id = idString(handle)
    val client = Client(handle, module, actual)
    clients[id] = client
    tick(id, client)
    return id
  }
  private fun get(module: Any, id: String): Client {
    val client = requireNotNull(clients[id])
    require(client.module === module && !client.failed && client.closing == null)
    return client
  }
  fun start(module: Any, id: String, kind: Int, json: String? = null): String {
    val client = get(module, id)
    require(json == null || json.toByteArray().size <= 1_048_576)
    val operation = if (kind == 2) client.poll.poll()
      else idString(ClientBindings.start(client.handle, kind, json?.toByteArray()))
    if (kind == 1) terminals.values.filter { it.client === client }.forEach { it.input.clear() }
    client.filter.expect(operation)
    return operation
  }
  fun afterPoll(module: Any, id: String, action: () -> String?, fenceV1: Boolean = false,
    done: (String?, Exception?) -> Unit) {
    val original = get(module, id)
    if (fenceV1 && original.profile == ClientProfile.V1) original.poll.prepareV1Fence()
    original.poll.submit({ check(get(module, id) === original); action() }, done)
  }
  fun changeViewBinding(module: Any, id: String, tab: String?, terminal: String, done: (String?, Exception?) -> Unit) {
    val client = get(module, id)
    val change = { if (tab == null) unbind(module, id, terminal) else bind(module, id, tab, terminal); null }
    if (client.profile == ClientProfile.V1 && (tab == null || !client.poll.failedPoll)) {
      // V1 bind only reserves a local parser; attach still waits for the poll.
      // An old parser can be released even after the carrier drops its poll.
      val error = try { change(); null } catch (cause: Exception) { cause }
      done(null, error)
    } else afterPoll(module, id, change, done = done)
  }
  fun afterTerminalPoll(terminal: String, action: () -> Unit, done: (Exception?) -> Unit) {
    val client = terminals[terminal]?.client
    if (client == null) { try { action(); done(null) } catch (error: Exception) { done(error) }; return }
    client.poll.submit({ check(client.closing == null); action(); null }) { _, error -> done(error) }
  }
  fun foreground(module: Any, value: Boolean) { clients.values.filter { it.module === module }.forEach { it.foreground = value } }
  fun profile(module: Any, id: String): String = get(module, id).profile.let {
    JSONObject().put("profile", it.code).put("apiCapabilities", it.apiCapabilities).toString()
  }
  private fun observer(module: Any, id: String): Client = get(module, id).also {
    require(it.profile == ClientProfile.OBSERVER_V2)
  }
  fun discoverObserver(module: Any, id: String, offerLegacy: Boolean): String {
    val client = observer(module, id)
    val operation = idString(ClientBindings.discoverObserver(client.handle, offerLegacy))
    client.filter.expect(operation)
    return operation
  }
  fun pairObserver(module: Any, id: String, attempt: String): String {
    val client = observer(module, id)
    require(attempt.length <= 36)
    val operation = idString(ClientBindings.pairObserver(client.handle, attempt.toByteArray()))
    client.filter.expect(operation)
    return operation
  }
  fun observerOperation(module: Any, id: String, kind: Int, json: String? = null): String {
    observer(module, id)
    require(kind in setOf(2, 3, 4))
    return start(module, id, kind, json)
  }
  fun reconnectObserver(module: Any, id: String, clientId: String) {
    val client = observer(module, id)
    require(clientId.length <= 36)
    ClientBindings.reconnectObserver(client.handle, clientId.toByteArray())
    client.poll.connection(false, "reconnecting")
    terminals.filterValues { it.client === client }.forEach { (terminal, binding) ->
      binding.input.clear(); binding.size = null; resetting.add(terminal)
    }
  }

  fun discoverManager(module: Any, id: String): String {
    val client = get(module, id); require(client.profile == ClientProfile.MANAGER_V2)
    val operation = idString(ClientBindings.discoverManager(client.handle)); client.filter.expect(operation); return operation
  }
  fun pairManager(module: Any, id: String, attempt: String): String {
    val client = get(module, id); require(client.profile == ClientProfile.MANAGER_V2 && attempt.length <= 36)
    val operation = idString(ClientBindings.pairManager(client.handle, attempt.toByteArray())); client.filter.expect(operation); return operation
  }
  fun reconnectManager(module: Any, id: String, identity: String) {
    val client = get(module, id); require(client.profile == ClientProfile.MANAGER_V2 && identity.length <= 36)
    ClientBindings.reconnectManager(client.handle, identity.toByteArray())
    client.poll.connection(false, "reconnecting")
    terminals.filterValues { it.client === client }.forEach { (_, binding) -> binding.input.clear(); binding.size = null }
  }
  fun managerOperation(module: Any, id: String, kind: Int, json: String? = null): String {
    require(get(module, id).profile == ClientProfile.MANAGER_V2 && kind in setOf(2, 3, 4)); return start(module, id, kind, json)
  }
  fun cancel(module: Any, id: String, operation: String) {
    ClientBindings.cancelOperation(get(module, id).handle, nativeId(operation))
  }
  fun bind(module: Any, id: String, tab: String, terminal: String) {
    val client = get(module, id)
    val handle = TerminalCoordinator.ownedHandle(module, terminal)
    require(terminals[terminal] == null)
    ClientBindings.bind(client.handle, tab.toByteArray(), handle)
    terminals[terminal] = Binding(client, handle)
    TerminalCoordinator.boundProfile(terminal, client.profile)
    if (client.profile == ClientProfile.MANAGER_V2) recovering.add(terminal)
  }
  fun unbind(module: Any, id: String, terminal: String) {
    val client = get(module, id)
    require(terminals[terminal]?.client === client)
    unbindTerminal(terminal)
  }
  fun unbindTerminal(terminal: String) {
    val binding = terminals[terminal] ?: return
    ClientBindings.unbind(binding.client.handle, nativeId(terminal))
    terminals.remove(terminal)
    TerminalCoordinator.boundProfile(terminal, ClientProfile.V1)
    resetting.remove(terminal)
    recovering.remove(terminal)
  }
  fun input(terminal: String, encoded: EncodedInput, release: Boolean = false) {
    val binding = requireNotNull(terminals[terminal])
    check(binding.client.closing == null)
    if (!binding.client.profile.allowsInputApi) throw NativeCallException(2)
    if (resetting.contains(terminal) || recovering.contains(terminal)) throw NativeInputStale()
    binding.input.submit(encoded, release)
  }
  fun inputNativeJson(terminal: String, json: String, release: Boolean) {
    val binding = requireNotNull(terminals[terminal]); require(binding.client.profile == ClientProfile.MANAGER_V2)
    check(binding.client.closing == null)
    if (resetting.contains(terminal) || recovering.contains(terminal)) throw NativeInputBusy()
    binding.input.submitNativeJson(json, release)
  }
  fun engineFailed(terminal: String) {
    terminals[terminal]?.client?.let { if (it.closing == null) it.failed = true }
  }
  fun consumed(terminal: String) {
    val binding = terminals[terminal] ?: return
    if (binding.client.closing != null) return
    when (ClientBindings.consumed(binding.client.handle, nativeId(terminal))) {
      0, 1, 3, 7 -> Unit // Busy retries next tick, stale while reconnecting grants no credit.
      else -> error("Terminal credit failed")
    }
  }
  fun resize(terminal: String, cols: Int, rows: Int, writable: Boolean) {
    val binding = terminals[terminal] ?: return
    val allowed = writable && binding.client.profile.allowsHostLayoutApi
    binding.size = if (allowed) Pair(cols, rows) else null
    if (!allowed) binding.input.dropResizes()
    resizePending(binding)
  }
  fun readEvent(module: Any, id: String): String? {
    val client = get(module, id)
    if (client.profile != ClientProfile.MANAGER_V2 && (resetting + recovering).any { terminals[it]?.client === client }) return null
    val value = client.events.pollFirst() ?: return null
    client.eventBytes -= value.toByteArray().size
    return value
  }
  fun close(module: Any, id: String, done: ((String?) -> Unit)? = null) {
    val client = requireNotNull(clients[id])
    require(client.module === module)
    val existing = client.closing
    if (existing != null) {
      if (existing.gate.uncertainClose) { done?.invoke("ENGINE"); return }
      if (done != null) { if (existing.callbacks.size >= 32) done("BUSY") else existing.callbacks.add(done) }
      return
    }
    client.closing = Closing(android.os.SystemClock.elapsedRealtime() + 3000, mutableListOf<(String?) -> Unit>().apply { if (done != null) add(done) })
    if (client.profile == ClientProfile.V1) client.poll.prepareV1Fence()
    client.poll.retire()
    if (client.failed) { client.failed = false; tick(id, client) }
  }
  private fun finishClose(id: String, client: Client) {
    val closing = requireNotNull(client.closing)
    try {
      if (!closing.gate.finish(client.poll.drained, ownerDrained(client)) { ClientBindings.close(client.handle, transport) }) return
      clients.remove(id)
      terminals.entries.filter { it.value.client === client }.forEach {
        resetting.remove(it.key); recovering.remove(it.key); TerminalCoordinator.boundProfile(it.key, ClientProfile.V1)
      }
      terminals.entries.removeAll { it.value.client === client }
      client.events.clear()
      val callbacks = closing.callbacks.toList(); closing.callbacks.clear(); callbacks.forEach { it(null) }
    } catch (_: Exception) {
      val callbacks = closing.callbacks.toList(); closing.callbacks.clear(); callbacks.forEach { it("ENGINE") }
    }
  }
  fun release(module: Any) {
    clients.filterValues { it.module === module }.keys.toList().forEach { close(module, it) }
  }
  fun released(module: Any) = clients.values.none { it.module === module }
  private fun resizePending(binding: Binding) {
    if (binding.client.closing != null || !binding.client.profile.allowsHostLayoutApi) return
    val (cols, rows) = binding.size ?: return
    try { if (binding.input.submitResize(cols, rows)) binding.size = null }
    catch (_: NativeInputBusy) { /* Keep the latest viewport until native admission has capacity. */ }
  }

  private fun ownerDrained(client: Client) = terminals.filterValues { it.client === client }.keys.all { TerminalCoordinator.queueDrained(it) }
  private fun tick(id: String, client: Client) {
    if (clients[id] !== client || client.failed) return
    try {
      val closing = client.closing
      if (closing != null && client.poll.drained && ownerDrained(client) && closing.operation == null) {
        try { closing.operation = idString(ClientBindings.start(client.handle, 4, null)) }
        catch (_: Exception) { /* Admission can be busy; bounded teardown retries before its deadline. */ }
      }
      ClientBindings.dispatch(client.handle, transport)
      var budget = 32
      while (budget-- > 0 && (closing != null || (client.events.size < 64 && client.eventBytes < 8_388_608))) {
        val bytes = ClientBindings.readEvent(client.handle) ?: break
        val event = JSONObject(bytes.toString(Charsets.UTF_8))
        val success = event.getInt("status") == 0 && event.optJSONObject("result")?.optBoolean("ok", true) != false
        client.poll.complete(event.getString("operationId"), event.getInt("status"), success)
        val connection = event.getJSONObject("connection")
        val connected = connection.getString("status") == "connected"
        val authority = connection.optJSONObject("binding")?.toString()
          ?: "${connection.optString("status")}/${connection.optString("connectionId")}"
        client.poll.connection(connected, authority)
        if (closing != null) {
          if (event.getString("operationId") == closing.operation) {
            closing.gate.disconnectCompleted(success)
            if (!success) { val callbacks = closing.callbacks.toList(); closing.callbacks.clear(); callbacks.forEach { it("ENGINE") } }
          }
          continue
        }
        val reset = event.optJSONArray("resetTerminals")
        if (reset != null) for (index in 0 until reset.length()) {
          val terminal = reset.getString(index)
          if (terminals[terminal]?.client === client) resetting.add(terminal)
        }
        // Reset handshake is complete before JS can observe the coarse operation result.
        event.remove("resetTerminals")
        event.remove("resetTickets")
        event.remove("resetGeometry")
        event.remove("recoveryReady")
        val encoded = client.filter.filter(event) ?: continue
        client.events.addLast(encoded)
        client.eventBytes += encoded.toByteArray().size
      }
      if (closing != null) {
        if (android.os.SystemClock.elapsedRealtime() >= closing.deadline && !closing.gate.disconnectSucceeded) {
          val callbacks = closing.callbacks.toList(); closing.callbacks.clear()
          callbacks.forEach { it("BUSY") }
        }
        if (client.poll.drained && ownerDrained(client) && closing.gate.disconnectSucceeded) finishClose(id, client)
        else TerminalCoordinator.later(10) { tick(id, client) }
        return
      }
      resetting.toList().filter { terminals[it]?.client === client }.forEach { terminal ->
        val handle = nativeId(terminal)
        val ticket = ClientBindings.resetTicket(client.handle, handle)
        if (ticket == 0L) resetting.remove(terminal)
        else {
          val grid = ResetGrid.decode(ClientBindings.resetGrid(client.handle, handle, ticket))
          if (grid != ResetGrid.Stale) {
            // Never combine a live ticket with dimensions from a queued event.
            terminals[terminal]?.input?.clear()
            if (grid is ResetGrid.Host) TerminalCoordinator.reset(terminal, grid.cols, grid.rows)
            else TerminalCoordinator.reset(terminal)
            when (ClientBindings.resetDone(client.handle, handle, ticket)) {
              0 -> {
                resetting.remove(terminal)
                if (grid is ResetGrid.Host) recovering.add(terminal) else recovering.remove(terminal)
              }
              3 -> Unit // A reconnect raced reset; no held bytes were installed.
              else -> error("Terminal reset failed")
            }
          }
        }
      }
      if (client.profile == ClientProfile.MANAGER_V2) {
        terminals.filterValues { it.client === client }.forEach { (terminal, binding) ->
          if (terminal !in recovering && !ClientBindings.recoveryReady(client.handle, nativeId(terminal))) {
            binding.input.clear()
            recovering.add(terminal)
            TerminalCoordinator.recoveryPending(terminal)
          }
        }
      }
      recovering.toList().filter { terminals[it]?.client === client && !resetting.contains(it) }.forEach { terminal ->
        if (ClientBindings.recoveryReady(client.handle, nativeId(terminal))) {
          TerminalCoordinator.fitAfterRecovery(terminal)
          recovering.remove(terminal)
        }
      }
      terminals.filterValues { it.client === client }.forEach { (terminal, binding) ->
        if (!resetting.contains(terminal) && !recovering.contains(terminal)) binding.input.drain()
        resizePending(binding)
      }
      client.poll.advance(client.foreground && (resetting + recovering).none { terminals[it]?.client === client }, ownerDrained(client))
    } catch (_: Exception) {
      val closing = client.closing
      if (closing != null) {
        val callbacks = closing.callbacks.toList(); closing.callbacks.clear(); callbacks.forEach { it("ENGINE") }
      } else client.failed = true
    }
    if (!client.failed) TerminalCoordinator.later(10) { tick(id, client) }
  }
}
