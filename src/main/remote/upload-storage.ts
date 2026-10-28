import { constants } from 'node:fs';
import { chmod, lstat, mkdir, open, realpath, readdir, rm, link, unlink } from 'node:fs/promises';
import type { FileHandle } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import { homedir } from 'node:os';
import path from 'node:path';
import { TERMINAL_UPLOAD_DEFAULT_MAX_BYTES, TERMINAL_UPLOAD_MAX_BYTES } from '../../shared/terminal-upload-schemas';
import { protectWindowsUploadDirectory } from './upload-permissions';

const ACTIVE_TTL = 5 * 60_000, SAVED_TTL = 24 * 60 * 60_000, MAX_ENTRIES = 128;
export interface UploadStorageOptions { root?: string; maxBytes?: number; now?: () => number }
export interface StoredUpload { id: string; directory: string; partial: string; destination: string; fileName: string; size: number; handle: FileHandle }
interface Entry { directory: string; size: number; touched: number; saved: boolean }
export function sanitizeUploadName(name: string): string {
  const leaf = name.split(/[\\/]/).at(-1) ?? '';
  let safe = leaf.replace(/[^a-zA-Z0-9._ -]/g, '_').slice(0, 120).replace(/^[ .]+|[ .]+$/g, '');
  if (!safe) safe = 'file';
  if (/^(?:con|prn|aux|nul|com[0-9]|lpt[0-9])(?:\.|$)/i.test(safe)) safe = `file_${safe}`;
  return safe;
}

/** A bounded private inbox. Completed files expire after 24 hours, including across restarts. */
export class UploadStorage {
  readonly maxBytes: number;
  readonly root: string;
  private readonly totalBytes: number;
  private readonly entries = new Map<string, Entry>();
  private readonly now: () => number;
  private ready?: Promise<void>;
  private rootIdentity?: { dev: number; ino: number };
  constructor(options: UploadStorageOptions = {}) {
    const configured = options.maxBytes ?? (process.env.SHELLFOX_UPLOAD_MAX_BYTES === undefined ? TERMINAL_UPLOAD_DEFAULT_MAX_BYTES : Number(process.env.SHELLFOX_UPLOAD_MAX_BYTES));
    if (!Number.isSafeInteger(configured) || configured < 1 || configured > TERMINAL_UPLOAD_MAX_BYTES) throw new Error('Invalid SHELLFOX_UPLOAD_MAX_BYTES.');
    const root = options.root ?? process.env.SHELLFOX_UPLOAD_ROOT ?? path.join(homedir(), '.shellfox', 'uploads');
    if (!path.isAbsolute(root) || /^[\\/]{2}/.test(root) || (process.platform === 'win32' && !/^[a-z]:[\\/]/i.test(root)) || /[\x00-\x1f\x7f]/.test(root)) throw new Error('SHELLFOX_UPLOAD_ROOT must be an absolute local path.');
    this.root = path.normalize(root); this.maxBytes = configured;
    this.totalBytes = Math.max(512 * 1024 * 1024, configured * 2); this.now = options.now ?? Date.now;
  }
  private async privateDirectory(directory: string): Promise<void> {
    const info = await lstat(directory);
    if (!info.isDirectory() || info.isSymbolicLink()) throw new Error('Upload directory is not a regular directory.');
    if (process.platform === 'win32') {
      await protectWindowsUploadDirectory(directory);
    } else {
      if (info.uid !== process.getuid?.()) throw new Error('Upload directory has a different owner.');
      await chmod(directory, 0o700);
    }
  }
  private async initialize(): Promise<void> {
    await mkdir(this.root, { recursive: true, mode: 0o700 });
    await this.privateDirectory(this.root);
    const info = await lstat(this.root);
    this.rootIdentity = { dev: info.dev, ino: info.ino };
    // Resolve parent aliases once, but never accept a symlink for the inbox itself.
    const canonical = path.normalize(await realpath(this.root));
    if ((process.platform === 'win32' ? canonical.toLowerCase() !== this.root.toLowerCase() : canonical !== this.root)) throw new Error('Upload root must be a canonical path.');
    const children = await readdir(this.root, { withFileTypes: true });
    if (children.length > MAX_ENTRIES) throw new Error('Upload directory entry limit exceeded.');
    for (const child of children) {
      if (!/^[0-9a-f-]{36}$/.test(child.name) || !child.isDirectory() || child.isSymbolicLink()) throw new Error('Unexpected upload directory entry.');
      const directory = path.join(this.root, child.name), files = await readdir(directory);
      if (files.length !== 1 || files[0] === '.partial') { await rm(directory, { recursive: true, force: true }); continue; }
      const name = files[0]!;
      if (sanitizeUploadName(name) !== name) throw new Error('Invalid saved upload name.');
      const file = await lstat(path.join(directory, name));
      if (!file.isFile() || file.isSymbolicLink() || file.size > TERMINAL_UPLOAD_MAX_BYTES) throw new Error('Invalid saved upload.');
      this.entries.set(child.name, { directory, size: file.size, touched: file.mtimeMs, saved: true });
    }
    await this.expire();
  }
  async check(): Promise<void> {
    await (this.ready ??= this.initialize());
    const info = await lstat(this.root);
    if (!info.isDirectory() || info.isSymbolicLink() || info.dev !== this.rootIdentity?.dev || info.ino !== this.rootIdentity.ino) throw new Error('Upload root changed.');
  }
  async create(fileName: string, size: number): Promise<StoredUpload> {
    await this.check();
    if (size > this.maxBytes || size < 0 || !Number.isSafeInteger(size)) throw new Error('File exceeds the host upload limit.');
    const total = [...this.entries.values()].reduce((sum, entry) => sum + entry.size, 0);
    if (this.entries.size >= MAX_ENTRIES || total + size > this.totalBytes) throw new Error('Host upload storage limit reached.');
    const id = randomUUID(), directory = path.join(this.root, id), safe = sanitizeUploadName(fileName);
    if (this.entries.has(id)) throw new Error('Upload identifier collision.');
    this.entries.set(id, { directory, size, touched: this.now(), saved: false });
    let created = false;
    try {
      await mkdir(directory, { mode: 0o700 }); created = true;
      const partial = path.join(directory, '.partial'), destination = path.join(directory, safe);
      const handle = await open(partial, constants.O_CREAT | constants.O_EXCL | constants.O_WRONLY | (constants.O_NOFOLLOW ?? 0), 0o600);
      return { id, directory, partial, destination, fileName: safe, size, handle };
    } catch (error) { if (created) await this.remove(id); else this.entries.delete(id); throw error; }
  }
  async verify(upload: StoredUpload): Promise<void> {
    await this.check();
    const directory = await lstat(upload.directory), file = await lstat(upload.partial), opened = await upload.handle.stat();
    if (!directory.isDirectory() || directory.isSymbolicLink() || !file.isFile() || file.isSymbolicLink() || file.dev !== opened.dev || file.ino !== opened.ino) throw new Error('Upload storage changed.');
  }
  touch(id: string): void { const entry = this.entries.get(id); if (entry) entry.touched = this.now(); }
  async save(upload: StoredUpload, authorized: () => boolean): Promise<void> {
    await this.verify(upload);
    await upload.handle.sync(); await upload.handle.close();
    if (!authorized()) throw new Error('Upload authorization changed.');
    // Hard-link publication is atomic and cannot replace an existing file or symlink.
    await link(upload.partial, upload.destination);
    if (!authorized()) { await unlink(upload.destination); throw new Error('Upload authorization changed.'); }
    await unlink(upload.partial);
    const entry = this.entries.get(upload.id);
    if (!entry || !authorized()) { await rm(upload.directory, { recursive: true, force: true }); throw new Error('Upload was cancelled.'); }
    entry.saved = true; entry.touched = this.now();
  }
  async remove(id: string): Promise<void> {
    const entry = this.entries.get(id); if (!entry) return;
    await rm(entry.directory, { recursive: true, force: true }); this.entries.delete(id);
  }
  async sweep(): Promise<void> {
    await this.check();
    await this.expire();
  }
  private async expire(): Promise<void> {
    for (const [id, entry] of this.entries) if (this.now() - entry.touched > (entry.saved ? SAVED_TTL : ACTIVE_TTL)) await this.remove(id);
  }
}
