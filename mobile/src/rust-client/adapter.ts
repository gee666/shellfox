import { z } from 'zod';
import { encodeTerminalConfiguration, type TerminalConfiguration } from './configure';
import { encodeTerminalInput, type TerminalInput } from './input';

import { positiveDecimalSchema, unsignedDecimalSchema } from './identifiers';

const decimal = positiveDecimalSchema;
export const terminalHandleSchema = decimal.brand<'TerminalHandle'>();
export type TerminalHandle = z.infer<typeof terminalHandleSchema>;
const sequence = unsignedDecimalSchema;
export const progressSchema = z.object({
  acceptedSequence: sequence,
  consumedSequence: sequence,
  queuedBytes: z.number().int().min(0).max(4 * 1024 * 1024),
  queuedChunks: z.number().int().min(0).max(64),
}).strict().refine((value) => unsignedDecimalSchema.safeParse(value.consumedSequence).success
  && unsignedDecimalSchema.safeParse(value.acceptedSequence).success
  && BigInt(value.consumedSequence) <= BigInt(value.acceptedSequence));
export type TerminalProgress = z.infer<typeof progressSchema>;

export interface NativeTerminalBinding {
  abiVersion(): number;
  createTerminal(cols: number, rows: number): Promise<string>;
  closeTerminal(terminalHandle: string): Promise<void>;
  progress(terminalHandle: string): Promise<string>;
  terminalInput(terminalHandle: string, json: string): Promise<void>;
  configureTerminal(terminalHandle: string, json: string): Promise<void>;
}

function dimensions(cols: number, rows: number): void {
  if (!Number.isInteger(cols) || !Number.isInteger(rows) || cols < 1 || rows < 1
    || cols > 512 || rows > 512 || cols * rows > 65536) {
    throw new Error('Invalid terminal dimensions.');
  }
}

/** UI lifecycle only. Terminal output, frame cells and network data stay native. */
export class RustTerminalAdapter {
  private readonly live = new Set<TerminalHandle>();
  private readonly closing = new Map<TerminalHandle, Promise<void>>();

  constructor(private readonly native: NativeTerminalBinding) {
    if (native.abiVersion() !== 1) throw new Error('Unsupported mobile Rust ABI.');
  }

  async create(cols: number, rows: number): Promise<TerminalHandle> {
    dimensions(cols, rows);
    const handle = terminalHandleSchema.parse(await this.native.createTerminal(cols, rows));
    if (this.live.has(handle)) throw new Error('Native terminal handle was reused.');
    this.live.add(handle);
    return handle;
  }

  async progress(handle: TerminalHandle): Promise<TerminalProgress> {
    this.requireLive(handle);
    const encoded = await this.native.progress(handle);
    if (encoded.length > 1024) throw new Error('Invalid terminal progress.');
    return progressSchema.parse(JSON.parse(encoded));
  }

  async input(handle: TerminalHandle, event: TerminalInput): Promise<void> {
    this.requireLive(handle);
    await this.native.terminalInput(handle, encodeTerminalInput(event));
  }

  async configure(handle: TerminalHandle, configuration: TerminalConfiguration): Promise<void> {
    this.requireLive(handle);
    await this.native.configureTerminal(handle, encodeTerminalConfiguration(configuration));
  }

  close(handle: TerminalHandle): Promise<void> {
    const closing = this.closing.get(handle);
    if (closing) return closing;
    if (!this.live.has(handle)) return Promise.resolve();
    const operation = Promise.resolve().then(() => this.native.closeTerminal(handle)).then(() => {
      this.live.delete(handle);
    }).finally(() => { this.closing.delete(handle); });
    this.closing.set(handle, operation);
    return operation;
  }

  private requireLive(handle: TerminalHandle): void {
    if (!this.live.has(handle) || this.closing.has(handle)) {
      throw new Error('Terminal is closed or closing.');
    }
  }
}
