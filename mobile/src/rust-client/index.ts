import NativeShellfoxRustClient from './specs/NativeShellfoxRustClient';
import { RustTerminalAdapter } from './adapter';

export { RustTerminalAdapter, canAdoptNativeTerminal } from './adapter';
export type { TerminalHandle, TerminalProgress, NativeAcceptance } from './adapter';
export type { TerminalInput, PhysicalKey, ModifierBits } from './input';
export { terminalConfiguration } from './configure';
export type { TerminalConfiguration } from './configure';
export { RustRemoteClient, rustRemoteMethods } from './remote-adapter';
export type { RustClientHandle, RustOperationId, RustRemoteMethod, NativeRemoteBinding } from './remote-adapter';

/** Optional during migration. Does not create an engine or start a network. */
export function nativeRustTerminalAdapter(): RustTerminalAdapter | null {
  return NativeShellfoxRustClient ? new RustTerminalAdapter(NativeShellfoxRustClient) : null;
}
