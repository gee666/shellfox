import { terminalHandleSchema, type TerminalHandle } from '../adapter';
import { encodeTerminalConfiguration, type TerminalConfiguration } from '../configure';
import { observerId, reply } from './primitives';
import { nativeAttachmentSchema, type ObserverAttachment, type ObserverDelivery } from './delivery';
import type { ObserverNativeBinding } from './native';

interface DeliveryHost {
  attach(tabId: string): Promise<unknown>;
  detach(delivery: ObserverDelivery): Promise<unknown>;
}

/** Delivery-only handle. No input, takeover, upload, resize or caller-ACK method. */
export class ObserverSurface {
  readonly readOnly = true;
  readonly layout = 'fixedCanonicalGrid';
  private closed = false;
  private closing?: Promise<void>;
  private metadata?: ObserverAttachment;

  private constructor(readonly tabId: string, readonly terminalHandle: TerminalHandle,
    private readonly native: ObserverNativeBinding, private readonly clientHandle: string,
    private readonly host: DeliveryHost, private readonly released: () => void,
  ) {}

  static async open(native: ObserverNativeBinding, clientHandle: string, tabId: string,
    configuration: TerminalConfiguration, host: DeliveryHost, released: () => void,
  ): Promise<ObserverSurface> {
    const tab = observerId.parse(tabId);
    // Placeholder only. Binding sends NO viewport geometry. Native must apply
    // the actual host recovery grid before feeding, then clip/pad locally.
    const terminal = terminalHandleSchema.parse(await native.createTerminal(80, 24));
    const view = new ObserverSurface(tab, terminal, native, clientHandle, host, released);
    let bound = false;
    try {
      await view.configure(configuration);
      await native.bindTerminal(clientHandle, tab, terminal); bound = true;
      await view.reattach();
      return view;
    } catch (error) {
      if (bound) await native.unbindTerminal(clientHandle, terminal).catch(() => undefined);
      await native.closeTerminal(terminal).catch(() => undefined);
      throw error;
    }
  }

  attachment(): ObserverAttachment | undefined {
    return this.metadata ? nativeAttachmentSchema.parse(this.metadata) : undefined;
  }
  async configure(configuration: TerminalConfiguration): Promise<void> {
    if (this.closed) throw new Error('Observer surface is closed.');
    await this.native.configureTerminal(this.terminalHandle, encodeTerminalConfiguration(configuration));
  }
  async reattach(): Promise<ObserverAttachment> {
    if (this.closed) throw new Error('Observer surface is closed.');
    const result = reply(nativeAttachmentSchema).parse(await this.host.attach(this.tabId));
    if (!result.ok) throw new Error(result.error.message);
    const tab = result.value.kind === 'attached' ? result.value.delivery.tabId : result.value.tabId;
    if (tab !== this.tabId) throw new Error('Observer attachment targets another tab.');
    this.metadata = result.value;
    return result.value;
  }
  close(): Promise<void> {
    if (this.closing) return this.closing;
    this.closed = true;
    this.closing = (async () => {
      try {
        if (this.metadata?.kind === 'attached') await this.host.detach(this.metadata.delivery).catch(() => undefined);
      } finally {
        try { await this.native.unbindTerminal(this.clientHandle, this.terminalHandle); }
        finally { await this.native.closeTerminal(this.terminalHandle); this.released(); }
      }
    })();
    return this.closing;
  }
}
