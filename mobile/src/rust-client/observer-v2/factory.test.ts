import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createProfileClient } from './factory';
import { ObserverTestNative, CLIENT } from './test-native';
import { RustHostClient } from '../rust-host-client';

const registry = vi.hoisted(() => ({ get: vi.fn(), enforcing: vi.fn(), view: vi.fn() }));
vi.mock('react-native', () => ({ TurboModuleRegistry: { get: registry.get, getEnforcing: registry.enforcing },
  UIManager: { hasViewManagerConfig: registry.view } }));
const options = { createClientId: () => CLIENT };
beforeEach(() => {
  vi.clearAllMocks();
  registry.get.mockReturnValue(null); registry.view.mockReturnValue(false);
  registry.enforcing.mockImplementation(() => { throw new Error('No enforcing module on an old binary.'); });
});
describe('separate observer profile factory', () => {
  it('defaults to native v1 without probing the observer extension', async () => {
    const native = new ObserverTestNative();
    registry.get.mockReturnValue(native); registry.view.mockReturnValue(true);
    const result = await createProfileClient('fixture-peer', options);
    expect(result.profile).toBe('v1'); expect(result.client).toBeInstanceOf(RustHostClient);
    expect(registry.get).toHaveBeenCalledExactlyOnceWith('NativeShellfoxRustClient');
    expect(registry.enforcing).not.toHaveBeenCalled();
    expect(native.observerV2AbiVersion).not.toHaveBeenCalled();
    expect(native.createClient).not.toHaveBeenCalled();
  });
  it('has no v1 fallback when a binary lacks the native terminal manager', async () => {
    registry.get.mockReturnValue(new ObserverTestNative());
    await expect(createProfileClient('fixture-peer', options)).rejects.toThrow('no Rust terminal view');
  });
  it('refuses observer adoption without both-platform evidence and never silently creates a v1 client', async () => {
    await expect(createProfileClient('fixture-peer', options, {
      profile: 'observer-v2', engine: { engine: 'rust-native', accepted: { android: true, ios: false } },
    })).rejects.toThrow('Android and iOS');
    expect(registry.get).not.toHaveBeenCalled();
  });
  it('requires extension ABI1 separately from wire version2 and returns a distinct inert facade', async () => {
    const native = new ObserverTestNative(); registry.get.mockReturnValue(native); registry.view.mockReturnValue(true);
    const selected = { profile: 'observer-v2', engine: { engine: 'rust-native', accepted: { android: true, ios: true } } } as const;
    native.observerV2AbiVersion.mockReturnValueOnce(0);
    await expect(createProfileClient('fixture-peer', options, selected)).rejects.toThrow('extension');
    const result = await createProfileClient('fixture-peer', options, selected);
    expect(result.profile).toBe('observer-v2');
    expect(result.client).not.toBeInstanceOf(RustHostClient);
    expect(native.createObserverV2Client).toHaveBeenCalledExactlyOnceWith('fixture-peer', CLIENT);
    expect(native.createClient).not.toHaveBeenCalled(); expect(native.discoverObserverV2).not.toHaveBeenCalled();
    expect(native.pairObserverV2).not.toHaveBeenCalled(); expect(registry.enforcing).not.toHaveBeenCalled();
    if (result.profile === 'observer-v2') await result.client.close();
  });
});
