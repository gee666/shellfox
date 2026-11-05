import path from 'node:path';
import { createHash } from 'node:crypto';
import { FILE_INLINE_TEXT_BYTES, FILE_INLINE_IMAGE_WRITE_BYTES, fileAssetMimeSchema } from '../../shared/file-schemas';
import { FileError } from './paths';
import type { FileRead, FileAsset } from '../../shared/file-contracts';

const types: Record<string, string> = {
  '.html': 'text/html', '.htm': 'text/html', '.css': 'text/css', '.js': 'text/javascript', '.mjs': 'text/javascript',
  '.json': 'application/json', '.md': 'text/markdown', '.txt': 'text/plain', '.svg': 'image/svg+xml',
  '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.gif': 'image/gif', '.webp': 'image/webp',
  '.ico': 'image/x-icon', '.avif': 'image/avif', '.woff': 'font/woff', '.woff2': 'font/woff2', '.ttf': 'font/ttf', '.otf': 'font/otf',
};
const assetExtensions = new Set(['.css', '.js', '.mjs', '.json', '.svg', '.png', '.jpg', '.jpeg', '.gif', '.webp', '.ico', '.avif', '.woff', '.woff2', '.ttf', '.otf']);
export const revision = (bytes: Buffer): string => createHash('sha256').update(bytes).digest('hex');
export const mimeType = (file: string): string => types[path.extname(file).toLowerCase()] ?? 'text/plain';
export function assetMimeType(file: string): FileAsset['mimeType'] {
  if (!assetExtensions.has(path.extname(file).toLowerCase())) throw new FileError('UNSUPPORTED', 'This file type cannot be embedded as a preview asset.');
  return fileAssetMimeSchema.parse(mimeType(file));
}
export function imageMimeType(bytes: Buffer): Extract<FileRead, { kind: 'image' }>['mimeType'] | null {
  if (bytes.subarray(0, 8).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]))) return 'image/png';
  if (bytes[0] === 255 && bytes[1] === 216 && bytes[2] === 255) return 'image/jpeg';
  if (/^GIF8[79]a$/.test(bytes.subarray(0, 6).toString('ascii'))) return 'image/gif';
  if (bytes.subarray(0, 4).toString('ascii') === 'RIFF' && bytes.subarray(8, 12).toString('ascii') === 'WEBP') return 'image/webp';
  if (bytes.subarray(0, 4).equals(Buffer.from([0, 0, 1, 0]))) return 'image/x-icon';
  if (bytes.subarray(4, 8).toString('ascii') === 'ftyp' && /^(avif|avis)$/.test(bytes.subarray(8, 12).toString('ascii'))) return 'image/avif';
  return null;
}
export function decodeText(bytes: Buffer): string {
  if (bytes.length > FILE_INLINE_TEXT_BYTES || bytes.includes(0)) throw new FileError('UNSUPPORTED', `Only UTF-8 text files up to ${FILE_INLINE_TEXT_BYTES / 1024} KiB can be edited.`);
  try { return new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes); }
  catch { throw new FileError('UNSUPPORTED', 'This file is not UTF-8 text.'); }
}
export function decodeImage(url: string, file: string): Buffer {
  const split = url.indexOf(','), type = url.slice(5, split - 7), bytes = Buffer.from(url.slice(split + 1), 'base64');
  if (!bytes.length || bytes.length > FILE_INLINE_IMAGE_WRITE_BYTES) throw new FileError('VALIDATION', 'Edited image exceeds the 512 KiB save limit.');
  if (!['image/png', 'image/jpeg', 'image/webp'].includes(type) || mimeType(file) !== type || imageMimeType(bytes) !== type) throw new FileError('VALIDATION', 'Image MIME type, header and file extension must match.');
  return bytes;
}
export const dataUrl = (type: string, bytes: Buffer): string => `data:${type};base64,${bytes.toString('base64')}`;
