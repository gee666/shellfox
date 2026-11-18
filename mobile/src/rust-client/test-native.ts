import { vi } from 'vitest';
import { z } from 'zod';
import { attachment } from '../client/test-fixtures';
import type { NativeTerminalBinding } from './adapter';
import type { NativeRemoteBinding } from './remote-adapter';
import type { NativeOperation } from './native-events';

/** Inert test-only native module. No account, socket, host process or filesystem. */
export class TestNative implements NativeRemoteBinding, NativeTerminalBinding {
  readonly events: NativeOperation[] = [];
  readonly polls: string[] = [];
  private nextOperation = 1;
  private nextClient = 1;
  private currentClient = '1';
  private readonly eventClients = new WeakMap<NativeOperation, string>();
  private nextTerminal = 100;
  connection: NativeOperation['connection'] = {
    status: 'connected', permission: 'approved', connectionId: '1', retryAfterMs: null,
  };
  result: (method: string, payload: unknown) => unknown = method => {
    if (method === 'attachTerminal') {
      const { chunks: _chunks, ...metadata } = attachment;
      return { ok: true, value: metadata };
    }
    if (method === 'takeTerminalControl') return { ok: true, value: { taken: true } };
    if (method === 'detachTerminal') return { ok: true, value: { detached: true } };
    return { ok: true, value: { cwd: '/home/work' } };
  };
  readonly abiVersion = vi.fn(() => 1);
  readonly createClient = vi.fn(async (_peer: string, _identity: string) => {
    this.currentClient = String(this.nextClient++); return this.currentClient;
  });
  readonly connect = vi.fn(async (handle: string) => this.complete({ ok: true, value: { paired: true, terminalControl: 'exclusive-v1' } }, 0, handle));
  readonly reconnect = vi.fn(async (handle: string) => {
    this.connection = { ...this.connection, connectionId: String(BigInt(this.connection.connectionId) + 1n) };
    return this.complete({ ok: true, value: { paired: true, terminalControl: 'exclusive-v1' } }, 0, handle);
  });
  readonly poll = vi.fn(async (_handle: string) => {
    const id = String(this.nextOperation++); this.polls.push(id); return id;
  });
  readonly operation = vi.fn(async (_handle: string, json: string) => {
    const input = z.object({ method: z.string(), payload: z.unknown() }).strict().parse(JSON.parse(json));
    return this.complete(this.result(input.method, input.payload), 0, _handle);
  });
  readonly cancel = vi.fn(async (_handle: string, _operation: string) => undefined);
  readonly bindTerminal = vi.fn(async (_client: string, _tab: string, _terminal: string) => undefined);
  readonly unbindTerminal = vi.fn(async (_client: string, _terminal: string) => undefined);
  readonly closeClient = vi.fn(async (_handle: string) => undefined);
  readonly readEvent = vi.fn(async (handle: string) => {
    const index = this.events.findIndex(event => this.eventClients.get(event) === handle);
    const event = index < 0 ? undefined : this.events.splice(index, 1)[0];
    return event ? JSON.stringify(event) : null;
  });
  readonly createTerminal = vi.fn(async (_cols: number, _rows: number) => String(this.nextTerminal++));
  readonly closeTerminal = vi.fn(async (_handle: string) => undefined);
  readonly configureTerminal = vi.fn(async (_handle: string, _json: string) => undefined);
  readonly terminalInput = vi.fn(async (_handle: string, _json: string) => undefined);
  readonly progress = vi.fn(async (_handle: string) => JSON.stringify({
    acceptedSequence: '0', consumedSequence: '0', queuedBytes: 0, queuedChunks: 0,
  }));

  complete(result: unknown, status = 0, client = this.currentClient): string {
    const id = String(this.nextOperation++); this.event(id, result, status, false, client); return id;
  }
  event(operationId: string, result: unknown, status = 0, internal = false, client = this.currentClient): void {
    const event = { operationId, internal, status, result, connection: { ...this.connection } };
    this.eventClients.set(event, client); this.events.push(event);
  }
}
