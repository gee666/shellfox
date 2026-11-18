import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ObserverV2Client } from './client';
import { ObserverTestNative, CLIENT, ATTEMPT, TAB, fixture } from './test-native';
import { coarseTerminalEventSchema } from './delivery';
import { reply, pairSelectionSchema } from './primitives';

const clients: ObserverV2Client[] = [];
async function setup() {
  const native = new ObserverTestNative();
  const client = await ObserverV2Client.create(native, 'fixture-peer', CLIENT);
  clients.push(client); return { native, client };
}
async function tick(ms = 120) { await vi.advanceTimersByTimeAsync(ms); }
async function connected() {
  const value = await setup();
  const discovery = value.client.discover(); await tick(); await discovery;
  const pair = value.client.pair(ATTEMPT); await tick(); await pair;
  return value;
}
beforeEach(() => vi.useFakeTimers());
afterEach(async () => {
  for (const client of clients.splice(0)) await client.close().catch(() => undefined);
  vi.useRealTimers();
});

describe('explicit delivery-only observer facade', () => {
  it('creates an inert observer and checks numeric API profile on client handles only', async () => {
    const { native, client } = await setup();
    expect(client.profile).toBe('observer-v2'); expect(client.apiCapabilities).toBe(1);
    expect(native.clientProfile).toHaveBeenCalledExactlyOnceWith('2');
    expect(native.discoverObserverV2).not.toHaveBeenCalled(); expect(native.pairObserverV2).not.toHaveBeenCalled();
    expect(native.createClient).not.toHaveBeenCalled(); expect(native.connect).not.toHaveBeenCalled();
    expect(native.createTerminal).not.toHaveBeenCalled();
    expect('input' in client).toBe(false); expect('resizeTerminal' in client).toBe(false);
    expect('acknowledgeTerminal' in client).toBe(false);
  });

  it('uses actual separate v2 snapshot facts, including unavailable entries, without v1 coercion', async () => {
    const { native, client } = await connected();
    const requested = client.request('getSnapshot', {}); await tick();
    const result = await requested;
    if (!result.ok) throw new Error('Expected fixture snapshot.');
    expect(result.version).toBe(2);
    expect(result.value.coverage).toEqual({ mode: 'complete', entryCount: 3 });
    expect(result.value.entries.some(entry => entry.kind === 'unavailable')).toBe(true);
    expect('sessions' in result.value).toBe(false);
    expect(native.operation).not.toHaveBeenCalled();
    expect(native.observerV2Operation).toHaveBeenCalledWith('2', '{"method":"getSnapshot","payload":{}}');
  });

  it('offers legacy visibly but never starts a v1 pair, attach, input or layout', async () => {
    const { native, client } = await setup();
    native.discoveryResult = { kind: 'legacyAvailable' };
    const discovering = client.discover('offer-legacy'); await tick();
    expect(await discovering).toEqual({ kind: 'legacyAvailable' });
    await expect(client.pair(ATTEMPT)).rejects.toThrow('available observer-v2');
    expect(native.discoverObserverV2).toHaveBeenCalledWith('2', true);
    expect(native.createClient).not.toHaveBeenCalled(); expect(native.connect).not.toHaveBeenCalled();
    expect('attachTerminal' in native).toBe(false); expect(native.operation).not.toHaveBeenCalled();
    expect(native.terminalInput).not.toHaveBeenCalled(); expect(native.observerV2Operation).not.toHaveBeenCalled();
  });

  it('does not infer HTTP 404 or legacy fallback from business errors or English messages', async () => {
    const { native, client } = await setup();
    native.discoveryResult = { version: 2, ok: false, error: { code: 'UNSUPPORTED',
      message: 'HTTP 404 not found', retryable: false, detail: { kind: 'application' } } };
    const discovering = client.discover('offer-legacy'); await tick();
    expect(await discovering).toEqual(native.discoveryResult);
    expect(native.createClient).not.toHaveBeenCalled(); expect(native.connect).not.toHaveBeenCalled();
    expect(native.pairObserverV2).not.toHaveBeenCalled();
  });

  it('checks pair echoes and permanently refuses legacy offers after any pair admission', async () => {
    const { native, client } = await connected();
    await client.reconnect('20000000-0000-4000-8000-000000000002');
    expect(client.getState()).toMatchObject({ status: 'reconnecting', binding: null });
    expect(native.pairObserverV2).toHaveBeenCalledTimes(1);
    native.discoveryResult = { kind: 'legacyAvailable' };
    const discovering = client.discover('offer-legacy'); const rejected = expect(discovering).rejects.toThrow('after v2 pair');
    await tick(); await rejected;
    const native2 = new ObserverTestNative();
    const invalid = await ObserverV2Client.create(native2, 'fixture-peer', CLIENT); clients.push(invalid);
    const discovery = invalid.discover(); await tick(); await discovery;
    const selection = reply(pairSelectionSchema).parse(fixture('pair-selection'));
    if (!selection.ok) throw new Error('Invalid fixture.');
    native2.pairResult = { ...selection, value: { ...selection.value, attemptId: '30000000-0000-4000-8000-000000000002' } };
    const pair = invalid.pair(ATTEMPT); const mismatch = expect(pair).rejects.toThrow('echo');
    await tick(); await mismatch;
    expect(invalid.getState().binding).toBeNull();
  });

  it('binds a passive canonical-grid surface and exposes no input/layout/caller ACK channel', async () => {
    const { native, client } = await connected();
    const opening = client.openTerminal(TAB, { foreground: '#ffffff', background: '#000000' });
    await tick(); const surface = await opening;
    expect(surface.readOnly).toBe(true); expect(surface.layout).toBe('fixedCanonicalGrid');
    expect(native.bindTerminal).toHaveBeenCalledWith('2', TAB, surface.terminalHandle);
    expect(native.clientProfile).not.toHaveBeenCalledWith(surface.terminalHandle);
    expect(native.observerV2Operation).toHaveBeenCalledWith('2', JSON.stringify({ method: 'attachTerminal', payload: { tabId: TAB, resume: null } }));
    expect(surface.attachment()?.kind).toBe('attached');
    expect(JSON.stringify(surface.attachment())).not.toContain('"data"');
    for (const name of ['input', 'resize', 'takeControl', 'acknowledge', 'upload']) expect(name in surface).toBe(false);
    expect(native.terminalInput).not.toHaveBeenCalled(); expect(native.operation).not.toHaveBeenCalled();
    const closing = surface.close(); await tick(); await closing;
    expect(native.unbindTerminal).toHaveBeenCalledWith('2', surface.terminalHandle);
    expect(native.closeTerminal).toHaveBeenCalledWith(surface.terminalHandle);
    expect(native.clientProfile).toHaveBeenCalledTimes(1);
  });

  it('does not turn source EOF into original-process exit and resolves native-internal polls once', async () => {
    const { native, client } = await connected(); const listener = vi.fn(); client.subscribe(listener);
    const sourceEnded = coarseTerminalEventSchema.parse(fixture('source-ended'));
    native.pollObserverV2.mockImplementationOnce(async () => native.observerComplete({ version: 2, ok: true,
      value: { events: [{ type: 'terminal', event: sourceEnded }] } }, true));
    const polling = client.poll(); await tick(); await polling;
    expect(listener).toHaveBeenCalledTimes(1);
    expect(listener).toHaveBeenCalledWith({ type: 'terminal', event: sourceEnded });
    expect('exitCode' in sourceEnded).toBe(false);
    expect(native.observerV2Operation).not.toHaveBeenCalled();
  });

  it('bounds an admitted pair and cannot reopen legacy fallback after cancellation', async () => {
    const { native, client } = await setup();
    const discovery = client.discover(); await tick(); await discovery;
    native.pairObserverV2.mockResolvedValueOnce('9000');
    const pairing = client.pair(ATTEMPT); const timedOut = expect(pairing).rejects.toThrow('timed out');
    await tick(35001); await timedOut;
    expect(native.cancel).toHaveBeenCalledWith('2', '9000');
    expect(native.pairObserverV2).toHaveBeenCalledTimes(1);
    native.discoveryResult = { kind: 'legacyAvailable' };
    const legacy = client.discover('offer-legacy'); const rejected = expect(legacy).rejects.toThrow('after v2 pair');
    await tick(); await rejected;
    expect(native.connect).not.toHaveBeenCalled();
  });

  it('bounds pending poll/withdrawal and never replays or auto-pairs on resume', async () => {
    const { native, client } = await connected();
    native.blockedPoll = true;
    const polling = client.poll(); const timedOut = expect(polling).rejects.toThrow('timed out');
    await tick(35001); await timedOut;
    expect(native.cancel).toHaveBeenCalledTimes(1);
    expect(native.pollObserverV2).toHaveBeenCalledTimes(1);
    native.blockedDisconnect = true;
    const suspending = client.suspend(); const withdrawal = expect(suspending).rejects.toThrow('timed out');
    await tick(35001); await withdrawal;
    expect(native.cancel).toHaveBeenCalledTimes(2);
    await client.reconnect('20000000-0000-4000-8000-000000000002');
    expect(native.pairObserverV2).toHaveBeenCalledTimes(1);
    expect(native.discoverObserverV2).toHaveBeenCalledTimes(1);
    expect(native.reconnectObserverV2).toHaveBeenCalledWith('2', '20000000-0000-4000-8000-000000000002');
  });
});
