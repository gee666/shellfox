import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createHostClient, setMobileClientEngine, mobileClientEngine } from './factory';
import { RemoteClient } from './client';
import { RustHostClient } from '../rust-client/rust-host-client';
import { TestNative } from '../rust-client/test-native';
import { ids, TestTransport } from './test-fixtures';

const registry = vi.hoisted(() => ({ get: vi.fn(), enforcing: vi.fn(), view: vi.fn() }));
vi.mock('react-native', () => ({
  TurboModuleRegistry: { get: registry.get, getEnforcing: registry.enforcing },
  UIManager: { hasViewManagerConfig: registry.view },
}));
const options = { createClientId: () => ids.client };
beforeEach(() => {
  vi.clearAllMocks(); setMobileClientEngine({ engine: 'webview-transition' });
  registry.get.mockReturnValue(null); registry.view.mockReturnValue(false);
  registry.enforcing.mockImplementation(() => { throw new Error('Old binary must not enforce native modules.'); });
});

describe('explicit mobile engine transition', () => {
  it('keeps the existing client as the explicit default without probing native modules', () => {
    const transport = new TestTransport();
    expect(mobileClientEngine()).toEqual({ engine: 'webview-transition' });
    expect(createHostClient('node_123', transport, options)).toBeInstanceOf(RemoteClient);
    expect(registry.get).not.toHaveBeenCalled(); expect(registry.enforcing).not.toHaveBeenCalled();
    expect(transport.request).not.toHaveBeenCalled();
  });

  it('does not adopt after only an Android compile or silently fall back when Rust was chosen', () => {
    expect(() => createHostClient('node_123', new TestTransport(), options, {
      engine: 'rust-native', accepted: { android: true, ios: false },
    })).toThrow('Android and iOS acceptance');
    expect(registry.get).not.toHaveBeenCalled();
    expect(() => createHostClient('node_123', new TestTransport(), options, {
      engine: 'rust-native', accepted: { android: true, ios: true },
    })).toThrow('no compatible Rust client');
    expect(registry.enforcing).not.toHaveBeenCalled();
  });

  it('requires both a compatible module and the Fabric view, but never connects on selection', () => {
    const native = new TestNative(); registry.get.mockReturnValue(native);
    const selected = { engine: 'rust-native', accepted: { android: true, ios: true } } as const;
    expect(() => createHostClient('node_123', new TestTransport(), options, selected)).toThrow('no Rust terminal view');
    registry.view.mockReturnValue(true);
    const transport = new TestTransport();
    expect(createHostClient('node_123', transport, options, selected)).toBeInstanceOf(RustHostClient);
    expect(native.createClient).not.toHaveBeenCalled(); expect(native.connect).not.toHaveBeenCalled();
    expect(transport.request).not.toHaveBeenCalled();
  });
});
