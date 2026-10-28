import { failure, success, type Result, type TerminalControlIdentity, type TerminalProfileDto } from '../../shared/contracts';
import { TERMINAL_UPLOAD_CHUNK_BYTES, uploadRequestSchemas, uploadResponseSchemas, type TerminalUploadMethod } from '../../shared/terminal-upload-schemas';
import type { EmbeddedSessionService } from '../terminal/service';
import type { TerminalControlView } from '../terminal/control';
import { UploadStorage, type StoredUpload, type UploadStorageOptions } from './upload-storage';
import { terminalUploadPath } from './upload-path';

interface Transfer { actor: string; identity: TerminalControlIdentity; file: StoredUpload; offset: number; touched: number; busy: boolean; cancelled: boolean; valid: () => boolean }
export interface UploadActor { id: string; delivery: TerminalControlView; valid: () => boolean }
export class TerminalUploads {
  private readonly storage: UploadStorage;
  private readonly transfers = new Map<string, Transfer>();
  private readonly starting = new Map<string, number>();
  private readonly cleanup = new Set<Promise<unknown>>();
  private disposed = false;
  private readonly operations = new Set<Promise<unknown>>();
  constructor(private readonly service: EmbeddedSessionService, options: UploadStorageOptions = {}) { this.storage = new UploadStorage(options); }
  private authorized(actor: UploadActor, identity: TerminalControlIdentity): Result<TerminalProfileDto> {
    if (this.disposed || !actor.valid()) return failure('ACCESS_DENIED', 'Upload connection is no longer approved.');
    const tab = this.service.repository.tab(identity.tabId), owned = this.service.backend.get(identity.tabId);
    if (!tab?.terminal || tab.operationId !== identity.generation || owned?.generation !== identity.generation || owned.state !== 'open') return failure('TARGET_LOST', 'This terminal is no longer running.');
    const profiles = this.service.getTerminalProfiles(); if (!profiles.ok) return profiles;
    const profile = profiles.value.profiles.find(p => p.id === tab.terminal?.profileId);
    if (!profile || !profile.available || profile.elevation) return failure('AUTH_FAILED', 'Uploads require a non-elevated terminal profile.');
    const control = actor.delivery.checkControl(identity); return control.ok ? success(profile) : control;
  }
  private track(task: Promise<unknown>): void {
    const safe = task.catch(() => {}); this.cleanup.add(safe); void safe.finally(() => this.cleanup.delete(safe));
  }
  private async remove(transfer: Transfer): Promise<void> {
    transfer.cancelled = true;
    await transfer.file.handle.close().catch(() => {});
    await this.storage.remove(transfer.file.id);
    this.transfers.delete(transfer.file.id);
  }
  cancelActor(actor: string, tabId?: string): void {
    for (const transfer of this.transfers.values()) if (transfer.actor === actor && (!tabId || transfer.identity.tabId === tabId)) {
      transfer.cancelled = true;
      if (!transfer.busy) this.track(this.remove(transfer));
    }
  }
  async sweep(): Promise<void> {
    for (const transfer of this.transfers.values()) if (Date.now() - transfer.touched > 5 * 60_000 || !transfer.valid()) {
      transfer.cancelled = true; if (!transfer.busy) await this.remove(transfer);
    }
    // Active handles are closed above before their directories can expire.
    await this.storage.sweep();
  }
  request(actor: UploadActor, method: TerminalUploadMethod, raw: unknown): Promise<unknown> {
    const operation = this.perform(actor, method, raw); this.operations.add(operation);
    void operation.finally(() => this.operations.delete(operation)).catch(() => {}); return operation;
  }
  private async perform(actor: UploadActor, method: TerminalUploadMethod, raw: unknown): Promise<unknown> {
    const parsed = uploadRequestSchemas[method].safeParse(raw);
    if (!parsed.success) {
      if (raw && typeof raw === 'object' && 'uploadId' in raw && typeof raw.uploadId === 'string') {
        const transfer = this.transfers.get(raw.uploadId);
        if (transfer?.actor === actor.id) { transfer.cancelled = true; if (!transfer.busy) await this.remove(transfer); }
      }
      return failure('VALIDATION', 'Invalid terminal upload request.');
    }
    const input = parsed.data;
    const permission = this.authorized(actor, input);
    if (method === 'beginTerminalUpload') {
      const begin = uploadRequestSchemas.beginTerminalUpload.parse(input);
      if (!permission.ok) return permission;
      if (begin.size > this.storage.maxBytes) return failure('VALIDATION', `File exceeds the host limit of ${this.storage.maxBytes} bytes.`);
      const count = [...this.transfers.values()].filter(t => t.actor === actor.id).length + (this.starting.get(actor.id) ?? 0);
      if (count >= 2 || this.transfers.size + [...this.starting.values()].reduce((sum, n) => sum + n, 0) >= 8) return failure('VALIDATION', 'Too many uploads. Finish or cancel an existing upload.');
      this.starting.set(actor.id, (this.starting.get(actor.id) ?? 0) + 1);
      let file: StoredUpload | undefined;
      try {
        file = await this.storage.create(begin.fileName, begin.size);
        const checked = this.authorized(actor, begin);
        if (!checked.ok) { await file.handle.close(); await this.storage.remove(file.id); return checked; }
        const identity = { tabId: begin.tabId, generation: begin.generation, controlEpoch: begin.controlEpoch };
        this.transfers.set(file.id, { actor: actor.id, identity, file, offset: 0, touched: Date.now(), busy: false, cancelled: false, valid: () => this.authorized(actor, identity).ok });
        return success({ uploadId: file.id, chunkBytes: TERMINAL_UPLOAD_CHUNK_BYTES });
      } catch {
        if (file) { await file.handle.close().catch(() => {}); await this.storage.remove(file.id).catch(() => {}); }
        return failure('STORAGE_FAILED', 'Host upload storage is unavailable or full.');
      } finally {
        const pending = (this.starting.get(actor.id) ?? 1) - 1;
        if (pending) this.starting.set(actor.id, pending); else this.starting.delete(actor.id);
      }
    }
    const identified = uploadRequestSchemas.finishTerminalUpload.parse({ tabId: input.tabId, generation: input.generation, controlEpoch: input.controlEpoch, uploadId: 'uploadId' in input ? input.uploadId : '' });
    const transfer = this.transfers.get(identified.uploadId);
    if (!transfer || transfer.actor !== actor.id) return failure('NOT_FOUND', 'Upload not found for this connection.');
    const same = transfer.identity.tabId === input.tabId && transfer.identity.generation === input.generation && transfer.identity.controlEpoch === input.controlEpoch;
    // An initiating client may still discard its own partial after losing control.
    if (method === 'cancelTerminalUpload' && same) {
      transfer.cancelled = true;
      if (!transfer.busy) await this.remove(transfer);
      return success({ cancelled: true });
    }
    if (!same || !permission.ok || transfer.cancelled) {
      transfer.cancelled = true; if (!transfer.busy) await this.remove(transfer);
      return !permission.ok ? permission : failure('TERMINAL_CONTROL_REQUIRED', 'Upload terminal control changed.');
    }
    if (transfer.busy) return failure('VALIDATION', 'An upload operation is already in progress.');
    transfer.busy = true; transfer.touched = Date.now(); this.storage.touch(transfer.file.id);
    const valid = () => !transfer.cancelled && this.authorized(actor, transfer.identity).ok;
    try {
      if (method === 'writeTerminalUpload') {
        const chunk = uploadRequestSchemas.writeTerminalUpload.parse(input), data = Buffer.from(chunk.data, 'base64');
        if (chunk.offset !== transfer.offset || transfer.offset + data.length > transfer.file.size) {
          await this.remove(transfer); return failure('VALIDATION', 'Upload offset or size does not match.');
        }
        await this.storage.verify(transfer.file);
        let written = 0;
        while (written < data.length) {
          if (!valid()) throw new Error('Upload authorization changed.');
          const result = await transfer.file.handle.write(data, written, data.length - written, transfer.offset + written);
          if (!result.bytesWritten) throw new Error('Upload write failed.');
          written += result.bytesWritten;
        }
        if (!valid()) throw new Error('Upload authorization changed.');
        transfer.offset += written; return success({ offset: transfer.offset });
      }
      if (transfer.offset !== transfer.file.size) { await this.remove(transfer); return failure('VALIDATION', 'Upload is incomplete.'); }
      const profile = this.authorized(actor, transfer.identity);
      if (!profile.ok) { await this.remove(transfer); return profile; }
      const resolved = await terminalUploadPath(transfer.file.destination, profile.value);
      if (!valid()) throw new Error('Upload authorization changed.');
      const response = uploadResponseSchemas.finishTerminalUpload.parse(success({ path: resolved, fileName: transfer.file.fileName, size: transfer.file.size }));
      await this.storage.save(transfer.file, valid);
      if (!valid()) throw new Error('Upload authorization changed.');
      this.transfers.delete(transfer.file.id); return response;
    } catch {
      await this.remove(transfer).catch(() => {});
      const checked = this.authorized(actor, transfer.identity);
      return checked.ok ? failure('STORAGE_FAILED', 'Upload could not be saved on the host.') : checked;
    } finally { transfer.busy = false; if (transfer.cancelled) await this.remove(transfer).catch(() => {}); }
  }
  async dispose(): Promise<void> {
    this.disposed = true;
    for (const transfer of this.transfers.values()) { transfer.cancelled = true; if (!transfer.busy) this.track(this.remove(transfer)); }
    await Promise.allSettled(this.operations);
    await Promise.allSettled(this.cleanup);
  }
}
