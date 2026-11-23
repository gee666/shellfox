import { CoarseOperations } from './coarse-operations';
import { nativeOperationSchema, type NativeOperation } from './native-events';
import { RustRemoteClient, parseRustOperationId } from './remote-adapter';

export { CoarseOperationError as NativeOperationError } from './coarse-operations';

/** Native signal that a client's readEvent has results. */
export interface NativeClientEvents {
  onClientEvent(handler: (clientHandle: string) => void): { remove(): void };
}

/** Reads the native client's coarse operation results. */
export class OperationPump extends CoarseOperations<NativeOperation> {
  constructor(readonly remote: RustRemoteClient, events: NativeClientEvents,
    observe: (event: NativeOperation) => void, failed: (error: Error) => void,
  ) {
    super({ read: () => remote.readEvent(nativeOperationSchema),
      cancel: id => remote.cancel(parseRustOperationId(id)), close: () => remote.close(),
      subscribe: listener => {
        const subscription = events.onClientEvent(handle => { if (handle === remote.handle) listener(); });
        return () => subscription.remove();
      },
    }, observe, failed);
  }
}
