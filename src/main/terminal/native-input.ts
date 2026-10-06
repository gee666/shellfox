// Pinned node-pty 1.1 adapter. Unknown addon internals cause a refusal, never an unbounded fallback.
export const MAX_NATIVE_INPUT_BYTES = 128 * 1024;
export function nativeQueuedInputBytes(pty: unknown, platform: NodeJS.Platform = process.platform): number | null {
  const raw = pty as { _isReady?: boolean; _agent?: { inSocket?: { writableLength?: number; destroyed?: boolean } }; _writeStream?: { _writeQueue?: { buffer?: Buffer; offset?: number }[] } };
  if (platform === 'win32') {
    if (raw._isReady !== true || raw._agent?.inSocket?.destroyed) return null;
    const length = raw._agent?.inSocket?.writableLength;
    return typeof length === 'number' && Number.isSafeInteger(length) && length >= 0 ? length : null;
  }
  const queue = raw._writeStream?._writeQueue;
  if (!Array.isArray(queue)) return null;
  if (queue.length >= 1024) return MAX_NATIVE_INPUT_BYTES;
  let bytes = 0;
  for (const task of queue) {
    if (!Buffer.isBuffer(task.buffer) || !Number.isSafeInteger(task.offset) || task.offset! < 0 || task.offset! > task.buffer.length) return null;
    // An in-flight prefix still occupies the native queue's backing Buffer until it is shifted.
    bytes += task.buffer.length;
  }
  return bytes;
}
