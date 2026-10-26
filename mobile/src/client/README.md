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

Native requests need a hard deadline of at most 30 seconds and a streamed response limit of 8 MiB before returning a string. Host event polls normally wait up to 20 seconds. Do not follow redirects. Request bodies must fit 1 MiB. The client validates encoded request sizes and decoded response sizes too, but cannot bound native allocations or cancel an in-flight request through this three-argument interface.

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
// On cleanup/backgrounding: offState(); offEvents(); client.stop();
```

`subscribeState` does not emit immediately. `getState()` returns `{ peerId, status, lastError, connectionId }`. `connectionId` is an opaque local epoch string, not the wire client UUID. It changes on every successful pairing. The states are `disconnected`, `connecting`, `awaiting-approval`, `connected`, `reconnecting` and `rejected`.

`connect()` starts background pairing and polling and is idempotent while running. Approval retries use the same attempt UUID. Host rejection stops retries. Calling `connect()` after rejection explicitly starts a new attempt, so discovery loops must not call it for rejected clients. `reconnect()` explicitly stops and starts a new attempt. `stop()` invalidates pending JS replies immediately and sends best-effort disconnect without killing host shells. A late native pairing response triggers another disconnect when its attempt is no longer current.

Automatic retries cover only pairing and event polling. A successful re-pair drops the host's previous delivery state and control leases. Refresh metadata, resend visible-session interest, and reattach terminal views after each new epoch. The client emits a terminal error for tracked attachments when their connection fails. It does not preserve terminal output or credentials.

## Session operations

```ts
const created = await client.createSession({ cwd: '/home/me/project', requestId: uuid() });
if (created.ok) await client.addTab({ sessionId: created.value.id });

await client.request('renameSession', { sessionId, title: 'work' });
await client.setVisibleSessions({ sessionIds: [sessionId] });
```

All allowed remote methods have convenience wrappers in `operations.ts`. `request(method, payload)` retains the shared method-specific payload and response types, validates both, and returns the shared `Result`. There are at most eight simultaneous operations per client, separate from its one event poll. Constructor configuration and invalid UUID factories can throw; operation failures return `Result` errors.

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

`terminalApi` and `subscribeTerminal` are stable bound references. The API contains `attachTerminal`, `writeTerminal`, `resizeTerminal`, `acknowledgeTerminal` and `detachTerminal`, with the existing `TerminalApi` signatures. `subscribeTerminal` emits the existing `TerminalEvent` directly, without a peer wrapper. `subscribe` emits the existing `RemoteHostEvent` envelope.

Terminal consumer responsibilities:

- Subscribe before attaching. Buffer events that arrive before the attach response, then reconcile them against its generation and `lastSequence`.
- Use the attachment's generation for input, resize, ACK and detach. Ignore stale-generation data and exit/error events. Activity events have no generation.
- Apply serialized screen snapshots at the attachment's `cols` and `rows`. Reset the displayed terminal on a fresh snapshot or changed generation. Use `afterSequence` only with matching retained terminal contents and generation.
- Replay attachment chunks, then ACK `lastSequence` only after xterm consumes them. ACK live output cumulatively after consumption too. Fetching or posting a chunk into the WebView is not consumption. The client never auto-ACKs. Unacknowledged output eventually exhausts the host's 128 KiB live credit.
- Respect `truncated` and replay-window errors. Detach and reattach to a current snapshot rather than repeatedly attaching with unacknowledged output.
- Detach hidden/unmounted terminal views. The host allows at most 16 attachments per view.
- Pass a changed epoch on reconnect so the component resets its attachment. The client does not auto-reattach or replay input.

Attaching is read-only and does not claim control. Write, resize and close claim or renew the host's 30-second lease; session deletion claims its live terminals. `FOCUS_DENIED` is returned unchanged and never automatically retried. There is no separate claim/heartbeat API. ACK and detach do not release a control lease; disconnect or expiry does. Local host input remains available.

## Checks

Run from the repository root:

```sh
pnpm typecheck
pnpm test:unit mobile/src/client
```

The root Vitest config includes mobile tests. Android/iOS native authentication, backgrounding and real WebView consumption still need device integration tests.
