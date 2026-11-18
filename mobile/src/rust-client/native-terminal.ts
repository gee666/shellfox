import { z } from 'zod';
import { idSchema, resultSchema, responseSchemas, terminalAttachmentSchema } from '../../../src/shared/schemas';
import type { TerminalEvent } from '../../../src/shared/contracts';
import type { RemoteRequestPayload } from '../../../src/shared/remote-contracts';
import type { MobileTerminalApi, TerminalBridgeState } from '../terminal/bridge';
import type { UploadIdentity } from '../terminal/upload';
import { TerminalOperationError } from '../terminal/errors';
import { RustTerminalAdapter, type TerminalHandle, type NativeTerminalBinding } from './adapter';
import type { TerminalInput } from './input';
import type { TerminalConfiguration } from './configure';
import type { RustRemoteClient, RustRemoteMethod } from './remote-adapter';
import type { NativeTerminalState } from './native-events';
import type {LayoutComparisonOutcome} from '../terminal/keyboard-retirement';

// The Rust v1 carrier serializes the typed attachment and removes only chunks.
// Reuse its wire fields, including lifetime and omission/null semantics. Chunk
// ordering/size checks belong to the wire decoder; retain the metadata boundary.
export const nativeAttachmentSchema = z.discriminatedUnion('control', [
  z.object(terminalAttachmentSchema.options[0].shape).omit({ chunks: true }).strict()
    .refine(value => value.firstSequence <= value.lastSequence + 1),
  terminalAttachmentSchema.options[1],
]);
export type NativeAttachment = z.infer<typeof nativeAttachmentSchema>;
export type NativeExecute = <M extends RustRemoteMethod>(method: M, payload: RemoteRequestPayload<M>) => Promise<unknown>;
/** Local presentation continuity only. Never grants readiness, ownership or input. */
export type NativeLayoutPresentation = Readonly<{kind:'layoutRecovery';phase:'pending'|'validated';identity:UploadIdentity}>
  | Readonly<{kind:'layoutRetired';phase?:never;identity?:never;outcome:LayoutComparisonOutcome}>;
export type NativeViewStateListener = (state:TerminalBridgeState,presentation?:NativeLayoutPresentation)=>void;
export type NativeViewSession = Pick<NativeTerminalSession, 'tabId' | 'handle' | 'readOnly' | 'getState' | 'nativeState' | 'getIdentity' | 'subscribeError' | 'configure' | 'takeControl' | 'input' | 'close'> & {
  subscribe(listener:NativeViewStateListener):()=>void;
};
export interface NativeViewDriver { open(tabId:string,readOnly:boolean,configuration:TerminalConfiguration):Promise<NativeViewSession> }
const drivers = new WeakMap<MobileTerminalApi, NativeTerminalDriver>();
export function nativeTerminalForApi(api: MobileTerminalApi): NativeTerminalDriver | undefined { return drivers.get(api); }
export function registerNativeTerminal(api: MobileTerminalApi, driver: NativeTerminalDriver): void { drivers.set(api, driver); }

export class NativeTerminalDriver {
  private readonly views = new Map<string, NativeTerminalSession>();
  private readonly opening = new Set<string>();
  private version = 0;
  constructor(private readonly native: NativeTerminalBinding,
    private readonly connection: () => Readonly<{ remote: RustRemoteClient; execute: NativeExecute }>,
  ) {}

  async open(tabId: string, readOnly: boolean, configuration: TerminalConfiguration): Promise<NativeTerminalSession> {
    idSchema.parse(tabId);
    if (this.views.has(tabId) || this.opening.has(tabId)) throw new Error('This terminal already has a native view.');
    const { remote, execute } = this.connection();
    const adapter = new RustTerminalAdapter(this.native);
    const version = this.version;
    this.opening.add(tabId);
    try {
      const handle = await adapter.create(80, 24);
      if (version !== this.version) {
        await adapter.close(handle); throw new Error('Native terminal connection changed.');
      }
      const view = new NativeTerminalSession(tabId, handle, readOnly, adapter, remote, execute,
        () => { if (this.views.get(tabId) === view) this.views.delete(tabId); });
      this.views.set(tabId, view);
      try { await view.initialize(configuration); return view; }
      catch (error) { await view.close().catch(() => undefined); throw error; }
    } finally { this.opening.delete(tabId); }
  }

  event(event: TerminalEvent): void { this.views.get(event.tabId)?.event(event); }
  recover(input: RemoteRequestPayload<'attachTerminal'>): void {
    const view = this.views.get(input.tabId);
    if (view) void view.attach(input).catch((error: unknown) => view.report(error));
  }
  async close(): Promise<void> {
    this.version++;
    await Promise.all([...this.views.values()].map((view) => view.close()));
  }
}

export class NativeTerminalSession {
  private bound = false;
  private initializing?: Promise<void>;
  private closed = false;
  private closing?: Promise<void>;
  private attaching?: Promise<void>;
  private metadata?: NativeAttachment;
  private state: TerminalBridgeState = 'attaching';
  private parserReady = false;
  private readonly listeners = new Set<(state: TerminalBridgeState) => void>();
  private readonly errors = new Set<(error: Error) => void>();

  constructor(readonly tabId: string, readonly handle: TerminalHandle, readonly readOnly: boolean,
    private readonly terminal: RustTerminalAdapter, private readonly remote: RustRemoteClient,
    private readonly execute: NativeExecute, private readonly released: () => void,
  ) {}

  async initialize(configuration: TerminalConfiguration): Promise<void> {
    this.initializing = (async () => {
      await this.configure(configuration);
      if (this.closed) return;
      await this.remote.bindTerminal(this.tabId, this.handle);
      this.bound = true;
      if (!this.closed) await this.attach();
    })();
    await this.initializing;
    if (this.closed) throw new Error('Native terminal is closed.');
  }

  getState(): TerminalBridgeState { return this.state; }
  nativeState(state: NativeTerminalState): void {
    if (!this.closed) this.parserReady = state === 'ready';
  }
  getIdentity(): UploadIdentity | null {
    const value = this.metadata;
    return !this.closed && !this.readOnly && this.parserReady && this.state === 'attached' && value?.control === 'owned'
      ? { tabId: value.tabId, generation: value.generation, controlEpoch: value.controlEpoch } : null;
  }
  subscribe(listener: (state: TerminalBridgeState) => void): () => void {
    this.listeners.add(listener); return () => { this.listeners.delete(listener); };
  }
  subscribeError(listener: (error: Error) => void): () => void {
    this.errors.add(listener); return () => { this.errors.delete(listener); };
  }
  report(cause: unknown): void {
    const error = cause instanceof Error ? cause : new Error('Native terminal failed.');
    for (const listener of this.errors) listener(error);
  }
  configure(value: TerminalConfiguration): Promise<void> { return this.terminal.configure(this.handle, value); }

  attach(input: RemoteRequestPayload<'attachTerminal'> = { tabId: this.tabId }): Promise<void> {
    if (this.closed) return Promise.reject(new Error('Native terminal is closed.'));
    if (this.attaching) return this.attaching;
    this.parserReady = false;
    this.change(this.metadata ? 'reconnecting' : 'attaching');
    const operation = (async () => {
      const result = resultSchema(nativeAttachmentSchema).parse(await this.execute('attachTerminal', input));
      if (!result.ok) throw new TerminalOperationError(result.error);
      if (result.value.tabId !== this.tabId) throw new Error('Native attachment targets another terminal.');
      this.metadata = result.value;
      if (this.closed) return;
      this.change(result.value.control === 'blocked' ? 'blocked' : result.value.state === 'closed' ? 'closed' : 'attached');
    })().finally(() => { if (this.attaching === operation) this.attaching = undefined; });
    this.attaching = operation;
    return operation;
  }

  async takeControl(): Promise<void> {
    if (this.readOnly || this.closed) return;
    await this.attaching;
    const metadata = this.metadata;
    if (!metadata) throw new Error('Terminal has no current generation.');
    const result = responseSchemas.takeTerminalControl.parse(await this.execute('takeTerminalControl', {
      tabId: this.tabId, generation: metadata.generation,
    }));
    if (!result.ok) throw new TerminalOperationError(result.error);
    await this.attach();
  }

  async input(event: TerminalInput): Promise<void> {
    if (this.closed || this.readOnly || !this.parserReady || this.state !== 'attached') return;
    await this.terminal.input(this.handle, event);
  }

  event(event: TerminalEvent): void {
    if (this.closed || event.type === 'data' || event.type === 'activity') return;
    if (this.metadata && 'generation' in event && event.generation !== this.metadata.generation) return;
    if (event.type === 'control') {
      if (this.metadata && event.controlEpoch < this.metadata.controlEpoch) return;
      if (event.control === 'blocked') this.change('blocked');
      else void this.attach().catch((error: unknown) => this.report(error));
    } else if (event.type === 'exit') this.change('closed');
    else if (event.type === 'error') { this.change('reconnecting'); this.report(new TerminalOperationError(event.error)); }
  }

  close(): Promise<void> {
    if (this.closing) return this.closing;
    this.closed = true;
    this.closing = (async () => {
      try {
        await this.initializing?.catch(() => undefined);
        await this.attaching?.catch(() => undefined);
        const metadata = this.metadata;
        if (this.bound && metadata) await this.execute('detachTerminal', {
          tabId: this.tabId, generation: metadata.generation,
        }).catch(() => undefined);
      } finally {
        try { if (this.bound) await this.remote.unbindTerminal(this.handle); }
        finally { await this.terminal.close(this.handle); this.released(); }
      }
    })();
    return this.closing;
  }

  private change(state: TerminalBridgeState): void {
    if (this.state === state) return;
    this.state = state;
    if (state !== 'attached') this.parserReady = false;
    for (const listener of this.listeners) listener(state);
  }
}
