import { z } from 'zod';
import { remotePeerIdSchema } from '../../../../src/shared/remote-schemas';
import { positiveDecimalSchema } from '../identifiers';
import { CoarseOperations } from '../coarse-operations';
import { clientProfileSchema, nativeObserverEventSchema, type NativeObserverEvent, type ObserverNativeBinding } from './native';
import { bindingSchema, discoveryOutcomeSchema, observerId, pairSelectionSchema, reply,
  disconnectedSchema, sameBinding, type ObserverBinding } from './primitives';
import { coarsePollSchema, type ObserverCoarseEvent, type ObserverDelivery } from './delivery';
import { managerCommand, parseObserverCommand, parseObserverResponse,
  type ObserverManagerMethod, type ObserverPayload, type ObserverResponse } from './methods';
import { ObserverSurface } from './surface';
import type { TerminalConfiguration } from '../configure';

const handleSchema = positiveDecimalSchema.brand<'ObserverClientHandle'>();
export type ObserverClientHandle = z.infer<typeof handleSchema>;
export type ObserverState = Readonly<{ status: NativeObserverEvent['connection']['status']; binding: ObserverBinding | null }>;
export type DiscoveryOutcome = z.infer<typeof discoveryOutcomeSchema>;

/** Explicit v2 profile. It intentionally does NOT implement the v1 HostClient DTO API. */
export class ObserverV2Client {
  readonly profile = 'observer-v2';
  readonly apiCapabilities = 1;
  private state: ObserverState = { status: 'disconnected', binding: null };
  private identity: string;
  private discovery?: DiscoveryOutcome;
  private fallbackClosed = false;
  private pairing = false;
  private polling = false;
  private withdrawing = false;
  private closed = false;
  private closing?: Promise<void>;
  private readonly pump: CoarseOperations<NativeObserverEvent>;
  private readonly listeners = new Set<(state: ObserverState) => void>();
  private readonly events = new Set<(event: ObserverCoarseEvent) => void>();
  private readonly surfaces = new Map<string, ObserverSurface>();
  private readonly opening = new Set<string>();

  private constructor(private readonly native: ObserverNativeBinding, readonly handle: ObserverClientHandle, identity: string) {
    this.identity = identity;
    this.pump = new CoarseOperations({ read: () => this.read(),
      cancel: operation => native.cancel(handle, operation), close: () => native.closeClient(handle),
    }, event => this.observe(event), () => this.setState({ status: 'reconnecting', binding: null }), 9);
    this.pump.start();
  }
  static async create(native: ObserverNativeBinding, peerId: string, clientId: string): Promise<ObserverV2Client> {
    if (native.abiVersion() !== 1 || native.observerV2AbiVersion() !== 1) throw new Error('Observer-v2 native ABI extension is unavailable.');
    const peer = remotePeerIdSchema.parse(peerId), identity = observerId.parse(clientId).toLowerCase();
    const handle = handleSchema.parse(await native.createObserverV2Client(peer, identity));
    try {
      const encoded = await native.clientProfile(handle);
      if (encoded.length > 1024) throw new Error('Invalid client profile.');
      const profile = clientProfileSchema.parse(JSON.parse(encoded));
      if (profile.profile !== 2 || profile.apiCapabilities !== 1) throw new Error('Native client is not delivery-only observer-v2.');
      return new ObserverV2Client(native, handle, identity);
    } catch (error) { await native.closeClient(handle).catch(() => undefined); throw error; }
  }

  getState(): ObserverState { return { ...this.state, binding: this.state.binding ? { ...this.state.binding } : null }; }
  subscribeState(listener: (state: ObserverState) => void): () => void {
    this.listeners.add(listener); return () => { this.listeners.delete(listener); };
  }
  subscribe(listener: (event: ObserverCoarseEvent) => void): () => void {
    this.events.add(listener); return () => { this.events.delete(listener); };
  }

  async discover(policy: 'never' | 'offer-legacy' = 'never'): Promise<DiscoveryOutcome> {
    this.live();
    const event = await this.pump.invoke(() => this.native.discoverObserverV2(this.handle, policy === 'offer-legacy'));
    const result = discoveryOutcomeSchema.parse(event.result);
    if ('kind' in result && result.kind === 'legacyAvailable' && this.fallbackClosed) throw new Error('Legacy offers are forbidden after v2 pair admission.');
    this.discovery = result;
    return result;
  }
  async pair(attemptId: string) {
    this.live();
    if (this.pairing) throw new Error('Observer pair is already pending.');
    const discovery = this.discovery;
    if (!discovery || !('kind' in discovery) || discovery.kind !== 'available') throw new Error('Discover an available observer-v2 profile before pairing.');
    const attempt = observerId.parse(attemptId).toLowerCase();
    this.fallbackClosed = true; this.pairing = true;
    try {
      const event = await this.pump.invoke(() => this.native.pairObserverV2(this.handle, attempt));
      const result = reply(pairSelectionSchema).parse(event.result);
      if (result.ok) {
        const selection = result.value;
        if (selection.clientId !== this.identity || selection.attemptId !== attempt || selection.ownerId !== discovery.discovery.ownerId) {
          throw new Error('Observer pair echo does not match the explicit offer.');
        }
        const binding = bindingSchema.parse({ clientId: selection.clientId, ownerId: selection.ownerId,
          connectionId: selection.connectionId, profile: selection.profile });
        if (!event.connection.binding || !sameBinding(binding, event.connection.binding)) throw new Error('Observer pair binding mismatch.');
        this.setState({ status: 'connected', binding });
      }
      return result;
    } finally { this.pairing = false; }
  }
  async request<M extends ObserverManagerMethod>(method: M, payload: ObserverPayload<M>): Promise<ObserverResponse<M>> {
    const command = managerCommand(method, payload);
    const binding = this.connected();
    const event = await this.pump.invoke(() => this.native.observerV2Operation(this.handle, command));
    const result = parseObserverResponse(method, event.result);
    if (result.ok) this.checkCurrent(event, binding);
    return result;
  }
  async poll() {
    const binding = this.connected();
    if (this.polling) throw new Error('Observer poll is already pending.');
    this.polling = true;
    try {
      const event = await this.pump.invoke(() => this.native.pollObserverV2(this.handle));
      const result = coarsePollSchema.parse(event.result);
      if (result.ok) { this.checkCurrent(event, binding); if (!event.internal) this.deliver(result.value.events); }
      return result;
    } finally { this.polling = false; }
  }
  async reconnect(clientId: string): Promise<void> {
    this.live();
    const identity = observerId.parse(clientId).toLowerCase();
    await this.native.reconnectObserverV2(this.handle, identity);
    this.identity = identity; this.discovery = undefined;
    this.setState({ status: 'reconnecting', binding: null });
    // This is a local fence. No discover, pair, attach or legacy connection is started.
  }
  async disconnect() {
    this.live();
    if (this.withdrawing) throw new Error('Observer withdrawal is already pending.');
    this.withdrawing = true;
    try {
      const event = await this.pump.invoke(() => this.native.disconnectObserverV2(this.handle));
      const result = reply(disconnectedSchema).parse(event.result);
      if (result.ok) this.setState({ status: 'disconnected', binding: null });
      return result;
    } finally { this.withdrawing = false; }
  }
  async suspend(): Promise<void> { await this.disconnect(); }

  async openTerminal(tabId: string, configuration: TerminalConfiguration): Promise<ObserverSurface> {
    const binding = this.connected();
    const tab = observerId.parse(tabId);
    if (this.surfaces.has(tab) || this.opening.has(tab)) throw new Error('Observer terminal is already bound.');
    this.opening.add(tab);
    try {
      let surface: ObserverSurface | undefined;
      const opened = await ObserverSurface.open(this.native, this.handle, tab, configuration, {
        attach: tabId => this.delivery('attachTerminal', { tabId, resume: null }),
        detach: delivery => this.delivery('detachTerminal', { delivery }),
      }, () => { if (this.surfaces.get(tab) === surface) this.surfaces.delete(tab); });
      surface = opened;
      const current = this.state.binding;
      if (this.closed || this.closing || !current || !sameBinding(current, binding)) {
        await opened.close().catch(() => undefined); throw new Error('Observer binding changed while opening surface.');
      }
      this.surfaces.set(tab, opened); return opened;
    } finally { this.opening.delete(tab); }
  }
  close(): Promise<void> {
    if (this.closing) return this.closing;
    this.closing = (async () => {
      try { await Promise.all([...this.surfaces.values()].map(surface => surface.close())); }
      finally {
        this.surfaces.clear(); this.closed = true;
        try { await this.pump.close(); }
        finally { this.setState({ status: 'disconnected', binding: null }); }
      }
    })();
    return this.closing;
  }
  private async delivery(method: 'attachTerminal' | 'detachTerminal', payload: unknown): Promise<unknown> {
    const binding = this.connected();
    const command = JSON.stringify(parseObserverCommand({ method, payload }));
    const event = await this.pump.invoke(() => this.native.observerV2Operation(this.handle, command));
    this.checkCurrent(event, binding); return event.result;
  }
  private connected(): ObserverBinding {
    this.live();
    if (!this.state.binding || this.state.status !== 'connected') throw new Error('Observer view is not connected.');
    return this.state.binding;
  }
  private live(): void {
    if (this.closed || this.closing) throw new Error('Observer client is closed or closing.');
    if (this.withdrawing) throw new Error('Observer withdrawal is pending.');
  }
  private checkCurrent(event: NativeObserverEvent, expected: ObserverBinding): void {
    const current = this.connected();
    if (!sameBinding(current, expected) || !event.connection.binding || !sameBinding(event.connection.binding, expected)) {
      throw new Error('Stale observer connection result.');
    }
  }
  private async read(): Promise<NativeObserverEvent | null> {
    const encoded = await this.native.readEvent(this.handle);
    if (encoded === null) return null;
    if (encoded.length > 8 * 1024 * 1024 + 65536 || new TextEncoder().encode(encoded).byteLength > 8 * 1024 * 1024 + 65536) {
      throw new Error('Observer native event exceeds limit.');
    }
    return nativeObserverEventSchema.parse(JSON.parse(encoded));
  }
  private observe(event: NativeObserverEvent): void {
    const connection = event.connection;
    if (!connection.binding) this.setState({ status: connection.status, binding: null });
    else if (this.state.binding && sameBinding(connection.binding, this.state.binding)) {
      this.setState({ status: connection.status, binding: connection.binding });
      if (event.internal && event.status === 0) {
        const poll = coarsePollSchema.safeParse(event.result);
        if (poll.success && poll.data.ok) this.deliver(poll.data.value.events);
      }
    }
  }
  private deliver(events: ReadonlyArray<ObserverCoarseEvent>): void {
    for (const event of events) for (const listener of this.events) { try { listener(event); } catch { /* Isolate presentation. */ } }
  }
  private setState(state: ObserverState): void {
    if (this.state.status === state.status && (this.state.binding === state.binding
      || this.state.binding && state.binding && sameBinding(this.state.binding, state.binding))) return;
    this.state = state;
    for (const listener of this.listeners) { try { listener(this.getState()); } catch { /* Isolate presentation. */ } }
  }
}
