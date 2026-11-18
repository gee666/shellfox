import { z } from 'zod';
import type { RemoteRequestPayload } from '../../../src/shared/remote-contracts';
import { remotePayloadSchemas, remotePeerIdSchema } from '../../../src/shared/remote-schemas';
import type { TerminalHandle } from './adapter';

import { positiveDecimalSchema } from './identifiers';

const identifier = positiveDecimalSchema;
const clientHandleSchema = identifier.brand<'RustClientHandle'>();
const operationIdSchema = identifier.brand<'RustOperationId'>();
export type RustClientHandle = z.infer<typeof clientHandleSchema>;
export type RustOperationId = z.infer<typeof operationIdSchema>;
export function parseRustOperationId(value: string): RustOperationId { return operationIdSchema.parse(value); }

// This is the current Rust allowlist, not every operation in the legacy client.
// Raw terminal writes and ACKs belong to the native encoder/consumption path.
export const rustRemoteMethods = [
  'getSnapshot', 'getTerminalProfiles', 'getHomeDirectory', 'createSession', 'addTab',
  'activateSession', 'renameSession', 'renameTab', 'reorderTabs', 'setSessionPinned',
  'settleSession', 'unsettleSession', 'deleteSession', 'clearSessionError',
  'setSessionEnv', 'setTerminalEnv', 'setVisibleSessions', 'attachTerminal',
  'takeTerminalControl', 'resizeTerminal', 'detachTerminal', 'closeTab',
  'listFiles', 'readFile', 'writeFile', 'writeImageFile', 'readFileAsset',
  'beginFileRead', 'readFileChunk', 'beginFileWrite', 'writeFileChunk',
  'finishFileWrite', 'cancelFileTransfer', 'getSessionStats',
  'resolveDirectory', 'createDirectory', 'completeDirectory',
  'beginTerminalUpload', 'writeTerminalUpload', 'finishTerminalUpload', 'cancelTerminalUpload',
] as const;
export type RustRemoteMethod = typeof rustRemoteMethods[number];

export interface NativeRemoteBinding {
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

/** Explicit remote operations only. Construction does not pair, poll or log in. */
export class RustRemoteClient {
  private closed = false;
  private closing: Promise<void> | undefined;

  private constructor(private readonly native: NativeRemoteBinding, readonly handle: RustClientHandle) {}

  static async create(native: NativeRemoteBinding, peerId: string, clientId: string): Promise<RustRemoteClient> {
    const peer = remotePeerIdSchema.parse(peerId);
    const identity = z.uuid().parse(clientId);
    const handle = clientHandleSchema.parse(await native.createClient(peer, identity));
    return new RustRemoteClient(native, handle);
  }

  connect(): Promise<RustOperationId> { return this.start(() => this.native.connect(this.handle)); }
  reconnect(): Promise<RustOperationId> { return this.start(() => this.native.reconnect(this.handle)); }
  poll(): Promise<RustOperationId> { return this.start(() => this.native.poll(this.handle)); }

  async operation<M extends RustRemoteMethod>(method: M, payload: RemoteRequestPayload<M>): Promise<RustOperationId> {
    this.requireLive();
    if (!rustRemoteMethods.some((allowed) => allowed === method)) throw new Error('Unsupported Rust remote method.');
    const validated = remotePayloadSchemas[method].parse(payload);
    const json = JSON.stringify({ method, payload: validated });
    if (new TextEncoder().encode(json).byteLength > 1024 * 1024) throw new Error('Remote request exceeds limit.');
    return this.start(() => this.native.operation(this.handle, json));
  }

  async cancel(operationId: RustOperationId): Promise<void> {
    this.requireLive();
    await this.native.cancel(this.handle, operationIdSchema.parse(operationId));
  }

  async bindTerminal(tabId: string, terminalHandle: TerminalHandle): Promise<void> {
    this.requireLive();
    await this.native.bindTerminal(this.handle, z.uuid().parse(tabId), terminalHandle);
  }

  async unbindTerminal(terminalHandle: TerminalHandle): Promise<void> {
    this.requireLive();
    await this.native.unbindTerminal(this.handle, terminalHandle);
  }

  /** Decode against the consumer's operation-specific schema, never cast JSON. */
  async readEvent<T>(schema: z.ZodType<T>): Promise<T | null> {
    this.requireLive();
    const json = await this.native.readEvent(this.handle);
    if (json === null) return null;
    // Native results omit terminal output. Leave 64KiB for coarse event metadata.
    if (json.length > 8 * 1024 * 1024 + 65536
      || new TextEncoder().encode(json).byteLength > 8 * 1024 * 1024 + 65536) {
      throw new Error('Remote event exceeds limit.');
    }
    return schema.parse(JSON.parse(json));
  }

  close(): Promise<void> {
    if (this.closing) return this.closing;
    if (this.closed) return Promise.resolve();
    this.closing = Promise.resolve().then(() => this.native.closeClient(this.handle)).then(() => {
      this.closed = true;
    }).finally(() => { this.closing = undefined; });
    return this.closing;
  }

  private async start(action: () => Promise<string>): Promise<RustOperationId> {
    this.requireLive();
    return operationIdSchema.parse(await action());
  }

  private requireLive(): void {
    if (this.closed || this.closing) throw new Error('Remote client is closed or closing.');
  }
}
