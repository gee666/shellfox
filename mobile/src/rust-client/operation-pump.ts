import { CoarseOperations } from './coarse-operations';
import { nativeOperationSchema, type NativeOperation } from './native-events';
import { RustRemoteClient, parseRustOperationId } from './remote-adapter';

export { CoarseOperationError as NativeOperationError } from './coarse-operations';

/** V1 adapter. Its strict DTOs and default behavior remain unchanged. */
export class OperationPump extends CoarseOperations<NativeOperation> {
  constructor(readonly remote: RustRemoteClient, observe: (event: NativeOperation) => void,
    failed: (error: Error) => void,
  ) {
    super({ read: () => remote.readEvent(nativeOperationSchema),
      cancel: id => remote.cancel(parseRustOperationId(id)), close: () => remote.close(),
    }, observe, failed);
  }
}
