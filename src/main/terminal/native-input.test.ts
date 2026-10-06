import { it, expect } from 'vitest';
import { nativeQueuedInputBytes, MAX_NATIVE_INPUT_BYTES } from './native-input';
it('counts retained Unix backing buffers, not merely unwritten tails', () => {
  const buffer = Buffer.alloc(MAX_NATIVE_INPUT_BYTES);
  expect(nativeQueuedInputBytes({ _writeStream: { _writeQueue: [{ buffer, offset: buffer.length - 1 }] } }, 'linux')).toBe(MAX_NATIVE_INPUT_BYTES);
  expect(nativeQueuedInputBytes({ _writeStream: { _writeQueue: [] } }, 'linux')).toBe(0);
});
it('bounds queue object count and refuses unknown or malformed addon layouts', () => {
  expect(nativeQueuedInputBytes({ _writeStream: { _writeQueue: Array.from({ length: 1024 }, () => ({ buffer: Buffer.from('x'), offset: 0 })) } }, 'linux')).toBe(MAX_NATIVE_INPUT_BYTES);
  expect(nativeQueuedInputBytes({}, 'linux')).toBeNull();
  expect(nativeQueuedInputBytes({ _writeStream: { _writeQueue: [{ buffer: Buffer.from('x'), offset: 3 }] } }, 'linux')).toBeNull();
});
it('uses real Windows socket backpressure and never appends to pre-ready deferred writes', () => {
  expect(nativeQueuedInputBytes({ _isReady: false, _agent: { inSocket: { writableLength: 0 } } }, 'win32')).toBeNull();
  expect(nativeQueuedInputBytes({ _isReady: true, _agent: { inSocket: { writableLength: 123 } } }, 'win32')).toBe(123);
  expect(nativeQueuedInputBytes({ _isReady: true, _agent: { inSocket: { writableLength: 0, destroyed: true } } }, 'win32')).toBeNull();
});
