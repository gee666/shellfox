import { randomUUID } from 'node:crypto';
import { ipcMain, type BrowserWindow, type IpcMainInvokeEvent } from 'electron';
import { failure, success, type CloseConfirmation } from '../shared/contracts';
import { closeConfirmationReadSchema, closeConfirmationResponseSchema, closeConfirmationSchema } from '../shared/close-confirmation-schemas';
import { isTrustedSender } from './ipc';

/** Environment variables alone must never suppress production shutdown approval. */
export function bypassCloseConfirmation(testBuild: boolean, launch: { userData?: string; backend?: 'fake' | 'real' }, optIn: string | undefined): boolean {
  return testBuild && !!launch.userData && !!launch.backend && optIn !== '1';
}

/** Only the main process can ask to quit/install. The renderer can answer one pending request. */
export class CloseConfirmationCoordinator {
  private pending: { confirmation: CloseConfirmation; resolve: (approved: boolean) => void; promise: Promise<boolean> } | null = null;
  private disposed = false;
  private rendererReady = false;
  constructor(private readonly window: BrowserWindow, private readonly rendererUrl: string, private readonly options: { autoApprove: boolean; present(): void }) {
    const trusted = (event: IpcMainInvokeEvent) => isTrustedSender(event, window, rendererUrl);
    ipcMain.handle('app:confirmation:get', (event, raw: unknown) => {
      if (!trusted(event)) return failure('AUTH_FAILED', 'Request did not come from the application renderer');
      if (!closeConfirmationReadSchema.safeParse(raw).success) return failure('VALIDATION', 'Invalid confirmation request');
      this.rendererReady = true;
      return success(this.pending?.confirmation ?? null);
    });
    ipcMain.handle('app:confirmation:respond', (event, raw: unknown) => {
      if (!trusted(event)) return failure('AUTH_FAILED', 'Request did not come from the application renderer');
      const parsed = closeConfirmationResponseSchema.safeParse(raw);
      if (!parsed.success) return failure('VALIDATION', 'Invalid confirmation response');
      if (!this.pending || parsed.data.id !== this.pending.confirmation.id) return failure('NOT_FOUND', 'This confirmation is no longer pending');
      this.finish(parsed.data.approved);
      return success({ accepted: true as const });
    });
    window.on('closed', this.cancel);
    window.webContents.on('render-process-gone', this.cancel);
    window.webContents.on('did-start-loading', this.cancel);
  }
  request(kind: CloseConfirmation['kind'], terminalCount: number): Promise<boolean> {
    if (this.disposed) return Promise.resolve(false);
    if (this.options.autoApprove) return Promise.resolve(true);
    if (!this.rendererReady || this.window.isDestroyed() || this.window.webContents.isDestroyed() || this.window.webContents.mainFrame.url !== this.rendererUrl) return Promise.resolve(false);
    if (this.pending) return this.pending.confirmation.kind === kind ? this.pending.promise : Promise.resolve(false);
    const parsed = closeConfirmationSchema.safeParse({ id: randomUUID(), kind, terminalCount });
    if (!parsed.success) return Promise.resolve(false);
    let resolve!: (approved: boolean) => void;
    const promise = new Promise<boolean>(accept => { resolve = accept; });
    this.pending = { confirmation: parsed.data, resolve, promise };
    try { this.publish(); this.options.present(); }
    catch { this.finish(false); }
    return promise;
  }
  private publish() {
    if (!this.window.isDestroyed() && !this.window.webContents.isDestroyed() && this.window.webContents.mainFrame.url === this.rendererUrl) {
      this.window.webContents.send('app:confirmation:changed', this.pending?.confirmation ?? null);
    }
  }
  private finish(approved: boolean) {
    const pending = this.pending;
    this.pending = null;
    pending?.resolve(approved);
    this.publish();
  }
  private cancel = () => { this.rendererReady = false; this.finish(false); };
  dispose() {
    if (this.disposed) return;
    this.disposed = true;
    this.finish(false);
    ipcMain.removeHandler('app:confirmation:get');
    ipcMain.removeHandler('app:confirmation:respond');
    this.window.removeListener('closed', this.cancel);
    this.window.webContents.removeListener('render-process-gone', this.cancel);
    this.window.webContents.removeListener('did-start-loading', this.cancel);
  }
}
