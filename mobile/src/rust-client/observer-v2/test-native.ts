import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { vi } from 'vitest';
import { z } from 'zod';
import { TestNative } from '../test-native';
import type { NativeObserverEvent, ObserverNativeBinding } from './native';
import { bindingSchema, discoverySchema, pairSelectionSchema, reply } from './primitives';
import { nativeAttachmentSchema } from './delivery';

export const fixture = (name: string): unknown => JSON.parse(readFileSync(resolve(
  'crates/shellfox-protocol/tests/fixtures/observer_v2', name + '.json'), 'utf8'));
export const CLIENT = '20000000-0000-4000-8000-000000000001';
export const ATTEMPT = '30000000-0000-4000-8000-000000000001';
export const TAB = '50000000-0000-4000-8000-000000000001';
const discovered = reply(discoverySchema).parse(fixture('discovery'));
const discoveryValue = (() => { if (!discovered.ok) throw new Error('Invalid fixture.'); return discovered.value; })();
const selected = reply(pairSelectionSchema).parse(fixture('pair-selection'));
if (!selected.ok) throw new Error('Invalid fixture.');
const binding = bindingSchema.parse(fixture('binding'));

/** Fixture-only native boundary. It performs no HTTP, Go, auth or host operations. */
export class ObserverTestNative extends TestNative implements ObserverNativeBinding {
  readonly observerEvents: NativeObserverEvent[] = [];
  readonly observerV2AbiVersion = vi.fn(() => 1);
  readonly clientProfile = vi.fn(async (_handle: string) => '{"profile":2,"apiCapabilities":1}');
  readonly createObserverV2Client = vi.fn(async (_peer: string, _identity: string) => '2');
  private next = 1000;
  connectionV2: NativeObserverEvent['connection'] = { status: 'disconnected', binding: null };
  discoveryResult: unknown = { kind: 'available', discovery: discoveryValue };
  pairResult: unknown = selected;
  blockedPoll = false;
  blockedDisconnect = false;
  operationResult: (method: string, payload: unknown) => unknown = (method) => {
    if (method === 'getSnapshot') {
      const value = z.object({ version: z.literal(2), binding: bindingSchema, ok: z.literal(true), value: z.unknown() }).strict().parse(fixture('snapshot'));
      const { binding: _binding, ...result } = value; return result;
    }
    if (method === 'attachTerminal') {
      const value = z.object({ version: z.literal(2), binding: bindingSchema, ok: z.literal(true), value: z.object({ recovery: z.object({ data: z.string() }).passthrough() }).passthrough() }).strict().parse(fixture('attachment'));
      const { data: _data, ...recovery } = value.value.recovery;
      return { version: 2, ok: true, value: nativeAttachmentSchema.parse({ ...value.value, recovery }) };
    }
    return { version: 2, ok: true, value: { detached: true } };
  };
  readonly discoverObserverV2 = vi.fn(async (_handle: string, _offerLegacy: boolean) => {
    this.connectionV2 = { status: 'discovered', binding: null };
    return this.observerComplete(this.discoveryResult);
  });
  readonly pairObserverV2 = vi.fn(async (_handle: string, _attempt: string) => {
    this.connectionV2 = { status: 'connected', binding: { ...binding } };
    return this.observerComplete(this.pairResult);
  });
  readonly observerV2Operation = vi.fn(async (_handle: string, json: string) => {
    const input = z.object({ method: z.string(), payload: z.unknown() }).strict().parse(JSON.parse(json));
    return this.observerComplete(this.operationResult(input.method, input.payload));
  });
  readonly pollObserverV2 = vi.fn(async (_handle: string) => {
    const operation = String(this.next++);
    if (!this.blockedPoll) this.observerEvent(operation, { version: 2, ok: true, value: { events: [] } }, true);
    return operation;
  });
  readonly reconnectObserverV2 = vi.fn(async (_handle: string, _identity: string) => {
    this.connectionV2 = { status: 'reconnecting', binding: null };
  });
  readonly disconnectObserverV2 = vi.fn(async (_handle: string) => {
    this.connectionV2 = { status: 'disconnected', binding: null };
    const operation = String(this.next++);
    if (!this.blockedDisconnect) this.observerEvent(operation, { version: 2, ok: true, value: { disconnected: true } });
    return operation;
  });
  override readonly readEvent = vi.fn(async (_handle: string) => {
    const event = this.observerEvents.shift(); return event ? JSON.stringify(event) : null;
  });
  observerComplete(result: unknown, internal = false): string {
    const operation = String(this.next++); this.observerEvent(operation, result, internal); return operation;
  }
  observerEvent(operationId: string, result: unknown, internal = false): void {
    this.observerEvents.push({ profile: 'observer-v2', operationId, internal, status: 0, result,
      connection: { ...this.connectionV2 } });
  }
}
