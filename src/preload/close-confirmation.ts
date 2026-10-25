import { ipcRenderer } from 'electron';
import type { CloseConfirmationApi } from '../shared/contracts';
import { failure } from '../shared/contracts';
import { closeConfirmationReadResultSchema, closeConfirmationResponseResultSchema, closeConfirmationResponseSchema, closeConfirmationStateSchema } from '../shared/close-confirmation-schemas';

export const closeConfirmationApi: CloseConfirmationApi = {
  async getCloseConfirmation() {
    try {
      const parsed = closeConfirmationReadResultSchema.safeParse(await ipcRenderer.invoke('app:confirmation:get', {}));
      return parsed.success ? parsed.data : failure('INTERNAL', 'Invalid confirmation state');
    } catch { return failure('INTERNAL', 'Could not read confirmation state', true); }
  },
  async respondCloseConfirmation(input) {
    if (!closeConfirmationResponseSchema.safeParse(input).success) return failure('VALIDATION', 'Invalid confirmation response');
    try {
      const parsed = closeConfirmationResponseResultSchema.safeParse(await ipcRenderer.invoke('app:confirmation:respond', input));
      return parsed.success ? parsed.data : failure('INTERNAL', 'Invalid confirmation response');
    } catch { return failure('INTERNAL', 'Could not answer confirmation', true); }
  },
  subscribeCloseConfirmation(listener) {
    if (typeof listener !== 'function') return () => {};
    const receive = (_event: unknown, value: unknown) => {
      const parsed = closeConfirmationStateSchema.safeParse(value);
      if (parsed.success) listener(parsed.data);
    };
    ipcRenderer.on('app:confirmation:changed', receive);
    return () => ipcRenderer.removeListener('app:confirmation:changed', receive);
  },
};
