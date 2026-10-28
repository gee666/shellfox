import { afterEach, describe, expect, it, vi } from 'vitest';
import { getEventListeners } from 'node:events';
import type { ProcessRule } from '../../shared/contracts';
import { ProcessTracker, type SnapshotProvider, type TrackingRoot, type TrackingSnapshot } from './tracking';

const root: TrackingRoot = {
  tabId: 'tab', sessionId: 'session', generation: 'generation', pid: 10,
  environment: 'local', distro: null, marker: 'marker',
};
const snapshot: TrackingSnapshot = {
  domain: 'test/local', environment: 'local', distro: null, complete: true,
  processes: [{ pid: 10, parentPid: 1, birth: 'birth:10', birthOrder: '10',
    executable: '/bin/bash', argv: ['bash'], accessible: true }],
};
const trackers: ProcessTracker[] = [];
afterEach(() => { trackers.splice(0).forEach(t => t.dispose()); vi.useRealTimers(); });

function deferredTracker() {
  const deliveries: Array<(snapshots: TrackingSnapshot[]) => void> = [];
  const provider = vi.fn<SnapshotProvider>(() => new Promise(resolve => { deliveries.push(resolve); }));
  const listener = vi.fn();
  const tracker = new ProcessTracker(listener, provider);
  trackers.push(tracker);
  return { tracker, provider, listener, deliveries };
}

// Resolves a provider and lets the tracker publish or start a replacement scan.
async function deliver(callback: (snapshots: TrackingSnapshot[]) => void) {
  callback([snapshot]);
  for (let i = 0; i < 8; i++) await Promise.resolve();
}

describe('tracking watch refreshes', () => {
  it('does not discard an in-flight scan for identical watches or start an immediate second scan', async () => {
    const f = deferredTracker();
    f.tracker.setWatch([root], []);
    for (let i = 0; i < 100; i++) f.tracker.setWatch([{ ...root }], []);
    await deliver(f.deliveries[0]);
    expect(f.provider).toHaveBeenCalledTimes(1);
    expect(f.listener).toHaveBeenCalledOnce();
    f.tracker.setWatch([{ ...root }], []);
    expect(f.provider).toHaveBeenCalledTimes(1);
  });

  it('keeps its own process-name rules and notices later edits to the caller array', async () => {
    const f = deferredTracker();
    const processNames = ['agent'];
    const rule: ProcessRule = { id: 'agent', label: 'Agent', enabled: true,
      executablePaths: [], executableBasenames: ['custom'], scriptPathSuffixes: [], processNames };
    const withChild: TrackingSnapshot = { ...snapshot, processes: [...snapshot.processes, {
      pid: 11, parentPid: 10, birth: 'birth:11', birthOrder: '11', accessible: true,
      executable: '/usr/bin/node', argv: ['node'],
    }] };
    f.tracker.setWatch([root], [rule]);
    processNames.push('node');
    f.deliveries[0]([withChild]);
    await f.tracker.poll();
    expect(f.listener.mock.lastCall![0]).toMatchObject([{ agents: 0 }]);
    f.tracker.setWatch([root], [rule]);
    expect(f.provider).toHaveBeenCalledTimes(2);
    f.deliveries[1]([withChild]);
    await f.tracker.poll();
    expect(f.listener.mock.lastCall![0]).toMatchObject([{ agents: 1 }]);
    f.tracker.setWatch([root], [{ ...rule, processNames: [...processNames] }]);
    expect(f.provider).toHaveBeenCalledTimes(2);
  });

  it('removes its cancellation listener after each successful poll', async () => {
    const provider = vi.fn<SnapshotProvider>(async () => [snapshot]);
    const tracker = new ProcessTracker(() => {}, provider);
    trackers.push(tracker);
    tracker.setWatch([root], []);
    for (let i = 0; i < 20; i++) {
      await tracker.poll();
      expect(getEventListeners(provider.mock.lastCall![1]!, 'abort')).toEqual([]);
    }
  });

  it('disposes a poll without waiting for an uncooperative provider', async () => {
    const f = deferredTracker();
    f.tracker.setWatch([root], []);
    let settled = false;
    const pending = f.tracker.poll().then(() => { settled = true; });
    f.tracker.dispose();
    for (let i = 0; i < 8; i++) await Promise.resolve();
    expect(settled).toBe(true);
    await deliver(f.deliveries[0]);
    await pending;
    expect(f.listener).not.toHaveBeenCalled();
  });
});
