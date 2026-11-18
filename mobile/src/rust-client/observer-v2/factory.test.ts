import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createProfileClient } from './factory';
import { ObserverTestNative, CLIENT } from './test-native';
import { TestTransport } from '../../client/test-fixtures';
import { RemoteClient } from '../../client/client';
import { setMobileClientEngine } from '../../client/factory';

const registry = vi.hoisted(() => ({ get: vi.fn(), enforcing: vi.fn(), view: vi.fn() }));
vi.mock('react-native', () => ({ TurboModuleRegistry: { get: registry.get, getEnforcing: registry.enforcing },
  UIManager: { hasViewManagerConfig: registry.view } }));
const options = { createClientId: () => CLIENT };
beforeEach(() => {
  vi.clearAllMocks(); setMobileClientEngine({ engine: 'webview-transition' });
  registry.get.mockReturnValue(null); registry.view.mockReturnValue(false);
  registry.enforcing.mockImplementation(() => { throw new Error('No enforcing module on an old binary.'); });
});
describe('separate observer profile factory', () => {
  it('keeps omitted profile/default engine on the original v1 path without probing the observer extension', async () => {
    const native = new ObserverTestNative(); const transport = new TestTransport(); registry.get.mockReturnValue(native);
    const result = await createProfileClient('fixture-peer', transport, options);
    expect(result.profile).toBe('v1'); expect(result.client).toBeInstanceOf(RemoteClient);
    expect(registry.get).not.toHaveBeenCalled(); expect(registry.enforcing).not.toHaveBeenCalled();
    expect(native.observerV2AbiVersion).not.toHaveBeenCalled(); expect(transport.request).not.toHaveBeenCalled();
  });
  it('refuses observer adoption without both-platform evidence and never silently creates a v1 client', async () => {
    const transport = new TestTransport();
    await expect(createProfileClient('fixture-peer', transport, options, {
      profile: 'observer-v2', engine: { engine: 'rust-native', accepted: { android: true, ios: false } },
    })).rejects.toThrow('Android and iOS');
    expect(registry.get).not.toHaveBeenCalled(); expect(transport.request).not.toHaveBeenCalled();
  });
  it('requires extension ABI1 separately from wire version2 and returns a distinct inert facade', async () => {
    const native = new ObserverTestNative(); registry.get.mockReturnValue(native); registry.view.mockReturnValue(true);
    const selected = { profile: 'observer-v2', engine: { engine: 'rust-native', accepted: { android: true, ios: true } } } as const;
    native.observerV2AbiVersion.mockReturnValueOnce(0);
    await expect(createProfileClient('fixture-peer', new TestTransport(), options, selected)).rejects.toThrow('extension');
    const result = await createProfileClient('fixture-peer', new TestTransport(), options, selected);
    expect(result.profile).toBe('observer-v2');
    expect(result.client).not.toBeInstanceOf(RemoteClient);
    expect(native.createObserverV2Client).toHaveBeenCalledExactlyOnceWith('fixture-peer', CLIENT);
    expect(native.createClient).not.toHaveBeenCalled(); expect(native.discoverObserverV2).not.toHaveBeenCalled();
    expect(native.pairObserverV2).not.toHaveBeenCalled(); expect(registry.enforcing).not.toHaveBeenCalled();
    if (result.profile === 'observer-v2') await result.client.close();
  });
});
