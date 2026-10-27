# Remote protocol client

`RemoteClient` is a remote-only TypeScript client. It imports contracts and Zod schemas from `../../../src/shared`, with no React Native, Electron, Node or UI imports in production code.

## Native boundary

```ts
interface NativeRemoteTransport {
  request(
    peerId: string,
    path: '/v1/pair' | '/v1/request' | '/v1/events' | '/v1/disconnect',
    body: string,
  ): Promise<string>;
}
```

Native code handles login, status, peer discovery and authenticated routing. Each request is a POST with the supplied JSON body. It must use the authenticated tailnet peer identity, not a hostname/IP or identity headers supplied by JavaScript. `clientId` identifies a host view; it is not an authentication token. Do not expose the desktop loopback backend secret to the mobile client.

Native requests need a hard deadline of at most 30 seconds and a streamed response limit of 8 MiB before returning a string. The JS watchdog allows 35 seconds so native deadline failures can cross the bridge before JS abandons the request. Host event polls normally wait up to 20 seconds. Do not follow redirects. Request bodies must fit 1 MiB. The client validates encoded request sizes and decoded response sizes too, but cannot bound native allocations or cancel an in-flight request through this three-argument interface.

The JS runtime must provide `TextEncoder`, `AbortController` and timers. The existing shared schemas also use `TextEncoder`. Supply any needed runtime polyfill before importing the client. Supply a cryptographically random UUID factory rather than a timestamp or persisted device ID.

## Connection and UI state

```ts
import { RemoteClient } from './client';

const client = new RemoteClient(peer.id, nativeNetwork, {
  createClientId: uuid,
});

setConnection(client.getState());
const offState = client.subscribeState(state => {
  setConnection(state);
  if (state.status === 'connected') void refreshSnapshot();
});
const offEvents = client.subscribe(event => {
  if (event.type === 'changed') void refreshSnapshot();
});

async function refreshSnapshot() {
  const result = await client.getSnapshot();
  if (result.ok) setSnapshot(result.value);
  else showError(result.error.message);
}

client.connect();
// On backgrounding: client.suspend(); On resume: client.connect();
// On explicit cancel or final cleanup: offState(); offEvents(); client.stop();
```

`subscribeState` does not emit immediately. `getState()` returns `{ peerId, status, lastError, connectionId }`. `connectionId` is an opaque local epoch string, not the wire client UUID. It changes on every successful pairing. The states are `disconnected`, `connecting`, `awaiting-approval`, `connected`, `reconnecting` and `rejected`.

`connect()` starts pairing and polling and resumes a suspended client with its existing attempt UUID. Approval retries use that same UUID. Host rejection stops retries. Calling `connect()` after rejection explicitly starts a new attempt, so discovery loops must not call it for rejected clients. `reconnect()` explicitly stops and starts a new attempt. `stop()` invalidates pending JS replies immediately and sends best-effort disconnect without killing host shells. A late native pairing response triggers another disconnect when its attempt was explicitly cancelled.

`suspend()` does not withdraw host approval requests. An in-flight pair may finish, but no retries, event polls or metadata reads start while suspended. Existing native calls drain before a replacement cycle starts, since the bridge cannot cancel them individually. Resuming an already connected view can wait for its outstanding event poll or operation, up to the native 30-second deadline. A pending approval has no event poll, so it resumes without that delay.

The host retains a delivered pending approval for one hour after its last request, without requiring screen-on heartbeats. Cancel request explicitly withdraws that attempt and removes auto-connect interest. A timeout is not a rejection; a later retry can create a fresh pending entry with the same active UUID. Explicit cancellation and host rejection still deny that UUID.

Attempt UUIDs are not persisted. If the OS kills the app process, its delivered host request may remain until the one-hour expiry or host approval/rejection. A restarted app creates a new attempt; Cancel request cancels that new UUID only, not an unknown request left by the previous process. Cancelling every attempt for the device would let late cleanup remove a newer request, so the client does not do that. Durable cancellation across process death would require a pending-attempt store bound to both authenticated local node identity and target peer.

Transport failures publish connection state before notifying terminal observers. `AUTH_FAILED` means the host view needs re-pairing, even if the host marks it nonretryable. Operation errors such as `FOCUS_DENIED`, `TARGET_LOST` and `NOT_FOUND` remain operation-scoped and do not re-pair a healthy connection. Local validation failures do not reconnect either.

Automatic retries cover only pairing and event polling. A successful re-pair drops the host's previous delivery registrations and terminal ownership. Refresh metadata, resend visible-session interest, and reattach terminal views after each new epoch. The client emits a terminal error for tracked attachments when their connection fails. It does not preserve terminal output or credentials.

## Session operations

```ts
const created = await client.createSession({ cwd: '/home/me/project', requestId: uuid() });
if (created.ok) await client.addTab({ sessionId: created.value.id });

await client.request('renameSession', { sessionId, title: 'work' });
await client.setVisibleSessions({ sessionIds: [sessionId] });
```

All allowed remote methods have convenience wrappers in `operations.ts`. `request(method, payload)` retains the shared method-specific payload and response types, validates both, and returns the shared `Result`. There are at most eight simultaneous operations per client, separate from its one event poll. `setVisibleSessions` sends one request at a time and replaces pending interest with the latest session IDs. Queued interest never crosses a connection epoch. Constructor configuration and invalid UUID factories can throw; operation failures return `Result` errors.

No mutation is automatically retried, including input, resize, tab creation and deletion. After a lost response the host may already have applied the operation. Refresh before deciding what to retry. Preserve `createSession.requestId` if deliberately retrying that creation.

## Terminal WebView

```tsx
<TerminalView
  api={client.terminalApi}
  subscribeTerminal={client.subscribeTerminal}
  tabId={tabId}
  connectionEpoch={connection.connectionId ?? ''}
  fontSize={fontSize}
  onError={onError}
/>
```

`terminalApi` and `subscribeTerminal` are stable bound references. The API contains `attachTerminal`, `takeTerminalControl`, `writeTerminal`, `resizeTerminal`, `acknowledgeTerminal` and `detachTerminal`, with the existing `TerminalApi` signatures. `subscribeTerminal` emits the existing `TerminalEvent` directly, without a peer wrapper. `subscribe` emits the existing `RemoteHostEvent` envelope.

Terminal consumer responsibilities:

- Subscribe before attaching. Buffer events that arrive before the attach response, then reconcile them against its generation and `lastSequence`.
- Use the attachment's generation and `controlEpoch` for input, resize and ACK. Detach uses identity only. Ignore stale-generation and stale-control-epoch data, exit and error events. Activity events have neither ownership epoch nor generation.
- An owned attachment has `control: 'owned'`, a `controlEpoch`, and the screen snapshot. Its raw replay chunks inherit the attachment's epoch. Apply the snapshot at its `cols` and `rows`. Reset the display on every fresh snapshot or changed generation.
- A blocked attachment has `control: 'blocked'`, identity and `controlEpoch` only. Clear previous contents and send no input, resize or ACK while blocked. A targeted owned control event requires a fresh full attachment before display and I/O resume.
- Replay owned attachment chunks, then ACK `lastSequence` only after xterm consumes them. ACK live output cumulatively after consumption too. Fetching or posting a chunk into the WebView is not consumption. The client never auto-ACKs. Unacknowledged output eventually exhausts the host's 128 KiB live credit.
- Respect `truncated` and replay-window errors. Detach and reattach to a current snapshot rather than repeatedly attaching with unacknowledged output.
- Detach hidden/unmounted terminal views. The host allows at most 16 attachments per view.
- Pass a changed epoch on reconnect so the component resets its attachment. The client does not auto-reattach or replay input.

`TerminalBridge` reports local delivery state through optional `onState`, with `attaching`, `attached`, `closed`, `reconnecting` and `blocked`. That state can be reconnecting while the client event poll is healthy. `TerminalOperationError` from `terminal/errors.ts` preserves the host `AppError` in `detail`; use its code rather than matching message text. Adjacent queued input is combined without adding a timer or changing bytes. Input is never generated by replaying terminal output. Pending cumulative ACKs coalesce to the latest rendered sequence.

Pi clears its editor on the first Ctrl+C and exits on a second within 500 milliseconds. Repeating Ctrl+C more slowly keeps clearing even an empty editor. Serialized request latency can exceed that window on a slow link; connection reuse and queued-write coalescing reduce avoidable delays, but do not guarantee Pi's timing over a high-latency network. No interrupt is injected or automatically repeated.

The first attached viewer owns an uncontrolled terminal. Other viewers remain registered but receive no terminal contents. `takeTerminalControl({ tabId, generation })` explicitly transfers ownership. Owner detach or disconnect promotes the next waiting viewer in arrival order. Ownership has no timeout, heartbeat or automatic resize renewal. `TERMINAL_CONTROL_REQUIRED` is returned unchanged and never automatically retried. Captured `controlEpoch` values prevent queued operations from a previous ownership period running after a later grant. The mobile UI clears the old WebView contents on loss and offers Take it on the blank blocked view. Hosts must advertise `terminalControl: 'exclusive-v1'` when pairing; older hosts require an upgrade.

## Checks

Run from the repository root:

```sh
pnpm typecheck
pnpm test:unit mobile/src/client
```

The root Vitest config includes mobile tests. Android/iOS native authentication, backgrounding and real WebView consumption still need device integration tests.
