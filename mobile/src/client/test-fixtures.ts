import { vi } from 'vitest';
import { success, type SessionDto, type TerminalAttachmentDto } from '../../../src/shared/contracts';
import type { RemoteHostEvent } from '../../../src/shared/remote-contracts';
import type { NativeRemoteTransport, RemotePath } from './protocol';

export const ids = {
  client: '10000000-0000-4000-8000-000000000001',
  nextClient: '10000000-0000-4000-8000-000000000002',
  tab: '20000000-0000-4000-8000-000000000001',
  generation: '30000000-0000-4000-8000-000000000001',
  session: '40000000-0000-4000-8000-000000000001',
};
export function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
export async function flush() { for (let i = 0; i < 30; i++) await Promise.resolve(); }

export const attachment: TerminalAttachmentDto = {
  tabId: ids.tab, sessionId: ids.session, generation: ids.generation,
  firstSequence: 1, lastSequence: 1,
  chunks: [{ type: 'data', tabId: ids.tab, generation: ids.generation, sequence: 1, data: 'hello' }],
  truncated: false, snapshot: true, state: 'open', exitCode: null, cols: 80, rows: 24, lifetime: 'app-owned',
};
export const session: SessionDto = {
  id: ids.session, title: 'work', cwd: '/home/work', adapterId: 'embedded-pty', shellId: 'bash',
  createdAt: '2026-01-01T00:00:00.000Z', updatedAt: '2026-01-01T00:00:00.000Z', settledAt: null,
  pinnedAt: null, status: 'waiting', activityStatus: 'waiting',
  counts: { running: 0, waiting: 0, unknown: 0, error: 0, closed: 0, agents: 0 },
  tabs: [], env: [], canFocus: false, canAddTab: true, controlReason: null, error: null,
};

/** Test-only stand-in at the native string/JSON boundary, with manually driven long polls. */
export class TestTransport implements NativeRemoteTransport {
  readonly polls: ReturnType<typeof deferred<string>>[] = [];
  pair: () => Promise<string> = async () => JSON.stringify(success({ paired: true }));
  operation: (body: string) => Promise<string> = async () => JSON.stringify(success({ cwd: '/home/work' }));
  readonly request = vi.fn(async (_peerId: string, path: RemotePath, body: string): Promise<string> => {
    if (path === '/v1/pair') return this.pair();
    if (path === '/v1/request') return this.operation(body);
    if (path === '/v1/disconnect') return JSON.stringify(success({ disconnected: true }));
    const poll = deferred<string>(); this.polls.push(poll); return poll.promise;
  });
  events(events: RemoteHostEvent[], index = this.polls.length - 1): void {
    this.polls[index].resolve(JSON.stringify(success({ events })));
  }
  calls(path: RemotePath): string[] {
    return this.request.mock.calls.filter(call => call[1] === path).map(call => call[2]);
  }
}
