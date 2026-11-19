import { TurboModuleRegistry, UIManager } from 'react-native';
import type { Spec } from './specs/NativeShellfoxRustClient';

export type EngineSelection =
  | Readonly<{ engine: 'webview-transition' }>
  | Readonly<{ engine: 'rust-native'; accepted: Readonly<{ android: boolean; ios: boolean }> }>;
export type NativeAvailability =
  | Readonly<{ available: true; native: Spec }>
  | Readonly<{ available: false; reason: string }>;

export function nativeAvailability(selection?: EngineSelection): NativeAvailability {
  if (selection?.engine === 'webview-transition') return { available: false, reason: 'WebView transition selected.' };
  if (selection && (!selection.accepted.android || !selection.accepted.ios)) {
    return { available: false, reason: 'Native terminals require Android and iOS acceptance.' };
  }
  try {
    const native = TurboModuleRegistry.get<Spec>('NativeShellfoxRustClient');
    if (!native || native.abiVersion() !== 1) return { available: false, reason: 'This binary has no compatible Rust client.' };
    const required: ReadonlyArray<keyof Spec> = ['createClient', 'readEvent', 'closeClient',
      'createTerminal', 'closeTerminal', 'bindTerminal', 'terminalInput', 'configureTerminal'];
    if (required.some((method) => typeof native[method] !== 'function')) {
      return { available: false, reason: 'This binary has incomplete Rust client bindings.' };
    }
    if (!UIManager.hasViewManagerConfig('ShellfoxRustTerminal')) {
      return { available: false, reason: 'This binary has no Rust terminal view.' };
    }
    return { available: true, native };
  } catch {
    return { available: false, reason: 'The native Rust client is unavailable in this binary.' };
  }
}
