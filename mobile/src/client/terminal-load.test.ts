import { afterEach, expect, it, vi } from 'vitest';
import { success, type TerminalEvent } from '../../../src/shared/contracts';
import { RemoteClient } from './client';
import { attachment, flush, ids, TestTransport } from './test-fixtures';
import { TerminalBridge } from '../terminal/bridge';
import type { HostMessage } from '../terminal/protocol';

const cleanups: Array<() => void> = [];
afterEach(() => { cleanups.splice(0).reverse().forEach(cleanup => cleanup()); vi.useRealTimers(); });

it('bounds terminal traffic over a slow native transport while typing, resizing and rendering', async () => {
  vi.useFakeTimers();
  const transport = new TestTransport();
  let active = 0, peak = 0;
  const methods: string[] = [];
  transport.operation = async body => {
    const input: { method: string } = JSON.parse(body);
    methods.push(input.method); peak = Math.max(peak, ++active);
    await new Promise(resolve => setTimeout(resolve, 1000));
    active--;
    switch (input.method) {
      case 'attachTerminal': return JSON.stringify(success(attachment));
      case 'writeTerminal': return JSON.stringify(success({ written: true }));
      case 'resizeTerminal': return JSON.stringify(success({ resized: true }));
      case 'acknowledgeTerminal': return JSON.stringify(success({ acknowledged: true }));
      case 'detachTerminal': return JSON.stringify(success({ detached: true }));
      case 'setVisibleSessions': return JSON.stringify(success({ configured: true }));
      default: throw new Error('Unexpected operation');
    }
  };
  const client = new RemoteClient('host-1', transport, { createClientId: () => ids.client });
  cleanups.push(() => client.stop()); client.connect(); await flush();
  const messages: HostMessage[] = [], onError = vi.fn();
  const bridge = new TerminalBridge({ api: client.terminalApi, subscribeTerminal: client.subscribeTerminal,
    tabId: ids.tab, viewId: 'load', fontSize: 14, onError, send: message => messages.push(message) });
  cleanups.push(() => bridge.dispose()); bridge.start();
  await vi.advanceTimersByTimeAsync(1000); await flush();
  const epoch = messages.find(message => message.type === 'reset')?.epoch;
  if (!epoch) throw new Error('Terminal did not attach');
  const render = () => {
    const output = messages.filter(message => message.type === 'output').at(-1);
    if (output) bridge.receive({ type: 'rendered', viewId: 'load', epoch, ticket: output.ticket });
  };
  render(); await flush();
  const visibility: Promise<unknown>[] = [];
  for (let round = 0; round < 15; round++) {
    for (let i = 0; i < 100; i++) {
      bridge.input('x');
      bridge.receive({ type: 'resize', viewId: 'load', epoch, cols: 80 + i % 20, rows: 24 + i % 10 });
      visibility.push(client.setVisibleSessions({ sessionIds: i % 2 ? [ids.session] : [] }));
    }
    const event: TerminalEvent = { type: 'data', tabId: ids.tab, generation: ids.generation, controlEpoch: 1,
      sequence: round + 2, data: 'live\r\n' };
    transport.events([{ type: 'terminal', event }]); await flush(); render();
    await vi.advanceTimersByTimeAsync(1000); await flush(); render();
  }
  await vi.advanceTimersByTimeAsync(6000); await flush();
  await Promise.all(visibility);
  expect(onError).not.toHaveBeenCalled();
  expect(client.getState().status).toBe('connected');
  expect(peak).toBeLessThanOrEqual(3); // Input/resize, delivery/ACK, and latest visibility.
  expect(methods.filter(method => method === 'attachTerminal')).toHaveLength(1);
  expect(methods.filter(method => method === 'setVisibleSessions').length).toBeLessThan(30);
  expect(methods.filter(method => method === 'writeTerminal').length).toBeLessThan(30);
  expect(methods.filter(method => method === 'resizeTerminal').length).toBeLessThan(30);
  expect(transport.calls('/v1/pair')).toHaveLength(1);
});
