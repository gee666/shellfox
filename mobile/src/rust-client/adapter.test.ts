import { describe, expect, it, vi } from 'vitest';
import { canAdoptNativeTerminal, RustTerminalAdapter, type NativeTerminalBinding } from './adapter';

function binding(): NativeTerminalBinding {
  return {
    abiVersion: vi.fn(() => 1),
    createTerminal: vi.fn(async () => '18446744073709551615'),
    closeTerminal: vi.fn(async () => undefined),
    terminalInput: vi.fn(async () => undefined),
    configureTerminal: vi.fn(async () => undefined),
    progress: vi.fn(async () => JSON.stringify({
      acceptedSequence: '10', consumedSequence: '9', queuedBytes: 100, queuedChunks: 1,
    })),
  };
}

describe('remote-only native terminal lifecycle', () => {
  it('does not create, connect or copy credentials during construction', () => {
    const native = binding();
    new RustTerminalAdapter(native);
    expect(native.createTerminal).not.toHaveBeenCalled();
    expect(Object.keys(native)).toEqual(['abiVersion', 'createTerminal', 'closeTerminal', 'terminalInput', 'configureTerminal', 'progress']);
  });

  it('keeps handles lossless and validates progress', async () => {
    const native = binding();
    const adapter = new RustTerminalAdapter(native);
    const handle = await adapter.create(80, 24);
    expect(handle).toBe('18446744073709551615');
    expect((await adapter.progress(handle)).consumedSequence).toBe('9');
    vi.mocked(native.progress).mockResolvedValueOnce('{"acceptedSequence":"1","consumedSequence":"2","queuedBytes":0,"queuedChunks":0}');
    await expect(adapter.progress(handle)).rejects.toThrow();
  });

  it('rejects unsafe handles, invalid dimensions, unsupported ABI', async () => {
    const native = binding();
    expect(() => new RustTerminalAdapter({ ...native, abiVersion: () => 2 })).toThrow();
    const adapter = new RustTerminalAdapter(native);
    await expect(adapter.create(512, 512)).rejects.toThrow();
    expect(native.createTerminal).not.toHaveBeenCalled();
    for (const invalid of ['0', '-1', '1.2', '01', '18446744073709551616', 'pointer:123']) {
      vi.mocked(native.createTerminal).mockResolvedValueOnce(invalid);
      await expect(adapter.create(80, 24)).rejects.toThrow();
    }
  });

  it('deduplicates close and retains failed close ownership for retry', async () => {
    const native = binding();
    const adapter = new RustTerminalAdapter(native);
    const handle = await adapter.create(80, 24);
    vi.mocked(native.closeTerminal).mockRejectedValueOnce(new Error('busy'));
    const first = adapter.close(handle);
    expect(adapter.close(handle)).toBe(first);
    await expect(first).rejects.toThrow('busy');
    await adapter.close(handle);
    await adapter.close(handle);
    expect(native.closeTerminal).toHaveBeenCalledTimes(2);
    await expect(adapter.progress(handle)).rejects.toThrow('closed');
  });

  it('does not adopt on one-platform compile evidence', () => {
    expect(canAdoptNativeTerminal({ android: true, ios: false })).toBe(false);
    expect(canAdoptNativeTerminal({ android: false, ios: true })).toBe(false);
    expect(canAdoptNativeTerminal({ android: true, ios: true })).toBe(true);
  });
});
