import { failure, success, type Result, type TerminalAttachmentDto, type TerminalBackendEvent, type TerminalControlIdentity, type TerminalEvent, type TerminalSnapshotDto } from '../../shared/contracts';
import { requestSchemas } from '../../shared/schemas';
import { TerminalDelivery } from './delivery';

interface Identity { tabId: string; sessionId: string; generation: string }
export interface TerminalControlPort {
  identity(tabId: string): Identity | undefined;
  snapshot(input: { tabId: string; generation?: string; afterSequence?: number }): Result<TerminalSnapshotDto>;
  write(input: { tabId: string; generation: string; data: string; userInitiated?: boolean }): Result<{ written: true }>;
  resize(input: { tabId: string; generation: string; cols: number; rows: number }): Result<{ resized: true }>;
}
interface Slot extends Identity { epoch: number; owner?: TerminalControlView; viewers: Map<TerminalControlView, { ready: boolean }> }
const denied = () => failure('TERMINAL_CONTROL_REQUIRED', 'This terminal is in use by another view. Take it to view or control it.');

/** One broker for every authenticated view of this service, regardless of transport. */
export class TerminalControlBroker {
  private readonly slots = new Map<string, Slot>();
  private readonly views = new Set<TerminalControlView>();
  private nextEpoch = 0;
  constructor(private readonly port: TerminalControlPort) {}
  createView(send: (event: TerminalEvent) => void, purge: (tabId: string) => void = () => {}): TerminalControlView {
    const view = new TerminalControlView(this, send, purge); this.views.add(view); return view;
  }
  reserve(view: TerminalControlView, identity: Identity): Result<{ reserved: true }> {
    // A disconnected creator must not leave a claim for a dead view.
    if (!this.views.has(view)) return success({ reserved: true });
    if (view.tabIds.size >= 16) return failure('VALIDATION', 'At most 16 terminal views may be attached.');
    if (this.slots.has(identity.tabId)) return failure('VALIDATION', 'Terminal already has a control slot.');
    this.slots.set(identity.tabId, { ...identity, epoch: ++this.nextEpoch, owner: view, viewers: new Map([[view, { ready: false }]]) });
    view.tabIds.add(identity.tabId);
    return success({ reserved: true });
  }
  snapshot(view: TerminalControlView, input: { tabId: string; generation?: string; afterSequence?: number }): Result<TerminalSnapshotDto> {
    const slot = this.slots.get(input.tabId);
    if (!slot || slot.owner !== view || !slot.viewers.get(view)?.ready) return denied();
    return this.port.snapshot(input);
  }
  attach(view: TerminalControlView, input: { tabId: string; generation?: string; afterSequence?: number }): Result<TerminalAttachmentDto> {
    if (!requestSchemas.attachTerminal.safeParse(input).success || !this.views.has(view)) return failure('VALIDATION', 'Invalid terminal attachment.');
    const rawIdentity = this.port.identity(input.tabId);
    if (!rawIdentity) return failure('NOT_FOUND', 'Terminal not found.');
    const identity = { tabId: rawIdentity.tabId, sessionId: rawIdentity.sessionId, generation: rawIdentity.generation };
    if (input.generation && input.generation !== identity.generation) return failure('TARGET_LOST', 'This terminal generation is stale.');
    let slot = this.slots.get(input.tabId);
    if (slot && slot.generation !== identity.generation) {
      for (const prior of slot.viewers.keys()) { prior.clear(slot); prior.tabIds.delete(slot.tabId); prior.send({ type: 'control', tabId: slot.tabId, generation: slot.generation, control: 'blocked', controlEpoch: ++this.nextEpoch }); }
      this.slots.delete(input.tabId); slot = undefined;
    }
    if (!slot) { slot = { ...identity, epoch: ++this.nextEpoch, viewers: new Map() }; this.slots.set(input.tabId, slot); }
    if (!slot.viewers.has(view) && view.tabIds.size >= 16) return failure('VALIDATION', 'At most 16 terminal views may be attached.');
    const fresh = !slot.viewers.has(view);
    if (fresh) { slot.viewers.set(view, { ready: false }); view.tabIds.add(slot.tabId); }
    if (slot.owner && slot.owner !== view) return success({ ...identity, control: 'blocked', controlEpoch: slot.epoch });
    const state = slot.viewers.get(view);
    const snapshot = this.port.snapshot({ tabId: slot.tabId, generation: slot.generation, ...(state?.ready && input.afterSequence !== undefined ? { afterSequence: input.afterSequence } : {}) });
    if (!snapshot.ok) { if (fresh) this.detach(view, { tabId: slot.tabId, generation: slot.generation }); return snapshot; }
    // Replacing credit never releases ownership. A fresh epoch fences old ACKs/input.
    if (state?.ready) slot.epoch = ++this.nextEpoch;
    view.clear(slot); slot.owner = view;
    const attached = view.delivery.attached(snapshot.value);
    if (!attached.ok) return attached;
    slot.viewers.set(view, { ready: true });
    return success({ ...snapshot.value, control: 'owned', controlEpoch: slot.epoch });
  }
  take(view: TerminalControlView, input: { tabId: string; generation: string }): Result<{ taken: true }> {
    if (!requestSchemas.takeTerminalControl.safeParse(input).success) return failure('VALIDATION', 'Invalid terminal takeover.');
    if (this.port.identity(input.tabId)?.generation !== input.generation) return failure('TARGET_LOST', 'This terminal generation is stale.');
    const slot = this.slots.get(input.tabId);
    if (!slot || slot.generation !== input.generation || !slot.viewers.has(view)) return failure('TARGET_LOST', 'Attach this terminal before taking control.');
    if (slot.owner !== view) this.transfer(slot, view);
    return success({ taken: true });
  }
  private transfer(slot: Slot, next: TerminalControlView): void {
    const previous = slot.owner; slot.epoch = ++this.nextEpoch; slot.owner = next;
    if (previous) {
      previous.clear(slot);
      slot.viewers.delete(previous); slot.viewers.set(previous, { ready: false });
      previous.send({ type: 'control', tabId: slot.tabId, generation: slot.generation, control: 'blocked', controlEpoch: slot.epoch });
    }
    next.clear(slot); slot.viewers.set(next, { ready: false });
    next.send({ type: 'control', tabId: slot.tabId, generation: slot.generation, control: 'owned', controlEpoch: slot.epoch });
  }
  private owned(view: TerminalControlView, input: TerminalControlIdentity): Slot | undefined {
    const slot = this.slots.get(input.tabId);
    return slot?.generation === input.generation && slot.epoch === input.controlEpoch && slot.owner === view && slot.viewers.get(view)?.ready ? slot : undefined;
  }
  checkControl(view: TerminalControlView, input: TerminalControlIdentity): Result<{ authorized: true }> {
    if (this.port.identity(input.tabId)?.generation !== input.generation) return failure('TARGET_LOST', 'This terminal generation is stale.');
    return this.owned(view, input) ? success({ authorized: true }) : denied();
  }
  write(view: TerminalControlView, input: TerminalControlIdentity & { data: string; userInitiated?: boolean }): Result<{ written: true }> {
    if (!requestSchemas.writeTerminal.safeParse(input).success) return failure('VALIDATION', 'Invalid terminal input.');
    if (this.port.identity(input.tabId)?.generation !== input.generation) return failure('TARGET_LOST', 'This terminal generation is stale.');
    if (!this.owned(view, input)) return denied();
    return this.port.write({ tabId: input.tabId, generation: input.generation, data: input.data, ...(input.userInitiated === undefined ? {} : { userInitiated: input.userInitiated }) });
  }
  resize(view: TerminalControlView, input: TerminalControlIdentity & { cols: number; rows: number }): Result<{ resized: true }> {
    if (!requestSchemas.resizeTerminal.safeParse(input).success) return failure('VALIDATION', 'Invalid terminal size.');
    if (this.port.identity(input.tabId)?.generation !== input.generation) return failure('TARGET_LOST', 'This terminal generation is stale.');
    if (!this.owned(view, input)) return denied();
    return this.port.resize({ tabId: input.tabId, generation: input.generation, cols: input.cols, rows: input.rows });
  }
  acknowledge(view: TerminalControlView, input: TerminalControlIdentity & { sequence: number }): Result<{ acknowledged: true }> {
    if (!requestSchemas.acknowledgeTerminal.safeParse(input).success) return failure('VALIDATION', 'Invalid terminal acknowledgement.');
    const slot = this.slots.get(input.tabId);
    if (!slot || slot.generation !== input.generation) return failure('TARGET_LOST', 'This terminal generation is stale.');
    // Drain the authorized epoch even if exit retention already removed its
    // saved tab. This grants no new attachment or access to another generation.
    if (!this.owned(view, input)) return denied();
    return view.delivery.acknowledge({ tabId: input.tabId, generation: input.generation, sequence: input.sequence });
  }
  detach(view: TerminalControlView, input: { tabId: string; generation: string }): Result<{ detached: true }> {
    if (!requestSchemas.detachTerminal.safeParse(input).success) return failure('VALIDATION', 'Invalid terminal detach.');
    const slot = this.slots.get(input.tabId);
    if (!slot) return success({ detached: true });
    if (slot.generation !== input.generation) return failure('TARGET_LOST', 'Stale terminal view generation.');
    view.clear(slot); slot.viewers.delete(view); view.tabIds.delete(slot.tabId);
    if (slot.owner === view) {
      slot.owner = undefined;
      const next = slot.viewers.keys().next().value;
      if (next) this.transfer(slot, next);
    }
    if (!slot.viewers.size) this.slots.delete(slot.tabId);
    return success({ detached: true });
  }
  event(event: TerminalBackendEvent): void {
    if (event.type === 'activity') { for (const view of this.views) view.send(event); return; }
    const slot = this.slots.get(event.tabId);
    if (!slot || slot.generation !== event.generation) return;
    if (event.type === 'exit') {
      // Waiting views have no replay credit to drain, but must learn that Take is no longer possible.
      for (const [view, state] of slot.viewers) if (view !== slot.owner || !state.ready) {
        view.clear(slot); view.tabIds.delete(slot.tabId); slot.viewers.delete(view);
        view.send({ ...event, controlEpoch: slot.epoch });
      }
      if (!slot.viewers.size) { this.slots.delete(slot.tabId); return; }
    }
    if (slot.owner && slot.viewers.get(slot.owner)?.ready) slot.owner.delivery.event(event);
  }
  deliver(view: TerminalControlView, event: TerminalBackendEvent): void {
    if (event.type === 'activity') return;
    const slot = this.slots.get(event.tabId);
    if (slot?.generation === event.generation && slot.owner === view && slot.viewers.get(view)?.ready) view.send({ ...event, controlEpoch: slot.epoch });
  }
  drop(view: TerminalControlView): void {
    for (const tabId of [...view.tabIds]) { const slot = this.slots.get(tabId); if (slot) this.detach(view, { tabId: slot.tabId, generation: slot.generation }); }
    view.delivery.dispose(); this.views.delete(view);
  }
  dispose(): void { for (const view of this.views) view.delivery.dispose(); this.views.clear(); this.slots.clear(); }
}

export class TerminalControlView {
  readonly tabIds = new Set<string>();
  readonly delivery: TerminalDelivery;
  constructor(private readonly broker: TerminalControlBroker, readonly send: (event: TerminalEvent) => void, private readonly purge: (tabId: string) => void) {
    this.delivery = new TerminalDelivery(input => broker.snapshot(this, input), event => broker.deliver(this, event));
  }
  clear(identity: { tabId: string; generation: string }): void { this.delivery.detach({ tabId: identity.tabId, generation: identity.generation }); this.purge(identity.tabId); }
  attach(input: { tabId: string; generation?: string; afterSequence?: number }) { return this.broker.attach(this, input); }
  take(input: { tabId: string; generation: string }) { return this.broker.take(this, input); }
  checkControl(input: TerminalControlIdentity) { return this.broker.checkControl(this, input); }
  write(input: TerminalControlIdentity & { data: string; userInitiated?: boolean }) { return this.broker.write(this, input); }
  resize(input: TerminalControlIdentity & { cols: number; rows: number }) { return this.broker.resize(this, input); }
  acknowledge(input: TerminalControlIdentity & { sequence: number }) { return this.broker.acknowledge(this, input); }
  detach(input: { tabId: string; generation: string }) { return this.broker.detach(this, input); }
  dispose(): void { this.broker.drop(this); }
}
