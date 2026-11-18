import { Platform, TurboModuleRegistry, UIManager } from 'react-native';
import type { EngineSelection } from '../availability';
import type { Spec } from '../specs/NativeShellfoxRustClient';
import { ManagerV2Client } from './client';
import type { ManagerNativeBinding } from './native';
/** Explicit profile only. This does not promote the shared default engine or
 * reinterpret Android acceptance as iOS acceptance. Each selected OS must pass.
 */
export async function createManagerProfileClient(peer:string,identity:string,engine:Extract<EngineSelection,{engine:'rust-native'}>):Promise<ManagerV2Client>{
  const accepted=Platform.OS==='android'?engine.accepted.android:Platform.OS==='ios'?engine.accepted.ios:false;
  if(!accepted)throw new Error('Manager native capability is not accepted on this platform.');
  const native=TurboModuleRegistry.get<Spec>('NativeShellfoxRustClient');
  const required:ReadonlyArray<keyof ManagerNativeBinding>=['managerV2AbiVersion','clientProfile','createManagerV2Client','discoverManagerV2','pairManagerV2','managerV2Operation','pollManagerV2','reconnectManagerV2','disconnectManagerV2','bindTerminal','terminalInput','readEvent','closeClient'];
  if(!native||native.abiVersion()!==1||required.some(method=>typeof native[method]!=='function')||native.managerV2AbiVersion()!==1||!UIManager.hasViewManagerConfig('ShellfoxRustTerminal'))throw new Error('This binary has no compatible Native0 Manager capability.');
  return ManagerV2Client.create(native,peer,identity);
}
