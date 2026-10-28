import { expect, it, vi } from 'vitest';
import { success, type TerminalEvent } from '../../../src/shared/contracts';
import { TerminalBridge, type MobileTerminalApi } from './bridge';
import { attachment, flush, ids } from '../client/test-fixtures';

it('exposes upload identity only for the current open owned attachment', async () => {
  let emit: (event: TerminalEvent) => void = () => {};
  const api: MobileTerminalApi = {
    attachTerminal: vi.fn(async () => success(attachment)),
    takeTerminalControl: vi.fn(async () => success({ taken: true })),
    writeTerminal: vi.fn(async () => success({ written: true })),
    resizeTerminal: vi.fn(async () => success({ resized: true })),
    acknowledgeTerminal: vi.fn(async () => success({ acknowledged: true })),
    detachTerminal: vi.fn(async () => success({ detached: true })),
  };
  const bridge = new TerminalBridge({
    api, tabId: ids.tab, viewId: 'upload', fontSize: 14,
    subscribeTerminal: listener => { emit = listener; return () => {}; }, send: () => {}, onError: () => {},
  });
  expect(bridge.getUploadIdentity()).toBeNull();
  bridge.start(); await flush();
  expect(bridge.getUploadIdentity()).toEqual({ tabId: ids.tab, generation: ids.generation, controlEpoch: 1 });
  emit({ type: 'control', tabId: ids.tab, generation: ids.generation, control: 'blocked', controlEpoch: 2 });
  expect(bridge.getUploadIdentity()).toBeNull();
  bridge.dispose();
  expect(bridge.getUploadIdentity()).toBeNull();
});
it('does not expose an upload identity for an exited shell', async () => {
  let emit: (event: TerminalEvent) => void = () => {};
  const api: MobileTerminalApi = {
    attachTerminal: vi.fn(async () => success(attachment)),
    takeTerminalControl: vi.fn(), writeTerminal: vi.fn(), resizeTerminal: vi.fn(), acknowledgeTerminal: vi.fn(),
    detachTerminal: vi.fn(async () => success({ detached: true })),
  };
  const bridge = new TerminalBridge({
    api, tabId: ids.tab, viewId: 'upload', fontSize: 14,
    subscribeTerminal: listener => { emit = listener; return () => {}; }, send: () => {}, onError: () => {},
  });
  bridge.start(); await flush();
  expect(bridge.getUploadIdentity()).not.toBeNull();
  emit({ type: 'exit', tabId: ids.tab, generation: ids.generation, controlEpoch: 1, lastSequence: attachment.lastSequence, exitCode: 0 });
  expect(bridge.getUploadIdentity()).toBeNull();
  bridge.dispose();
});
