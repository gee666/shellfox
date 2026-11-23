import type { CodegenTypes, TurboModule } from 'react-native';
import { TurboModuleRegistry } from 'react-native';

export interface Spec extends TurboModule {
  /** Client handle whose readEvent has results. Coalesced until readEvent returns null. */
  readonly onClientEvent: CodegenTypes.EventEmitter<string>;
  abiVersion(): number;
  createTerminal(cols: number, rows: number): Promise<string>;
  closeTerminal(terminalHandle: string): Promise<void>;
  progress(terminalHandle: string): Promise<string>;
  terminalInput(terminalHandle: string, json: string): Promise<void>;
  configureTerminal(terminalHandle: string, json: string): Promise<void>;
  createClient(peerId: string, clientId: string): Promise<string>;
  connect(clientHandle: string): Promise<string>;
  reconnect(clientHandle: string): Promise<string>;
  poll(clientHandle: string): Promise<string>;
  operation(clientHandle: string, json: string): Promise<string>;
  cancel(clientHandle: string, operationId: string): Promise<void>;
  bindTerminal(clientHandle: string, tabId: string, terminalHandle: string): Promise<void>;
  unbindTerminal(clientHandle: string, terminalHandle: string): Promise<void>;
  readEvent(clientHandle: string): Promise<string | null>;
  closeClient(clientHandle: string): Promise<void>;
}

export default TurboModuleRegistry.get<Spec>('NativeShellfoxRustClient');
