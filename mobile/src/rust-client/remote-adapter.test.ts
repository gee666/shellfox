import { describe, expect, it, vi } from 'vitest';
import { z } from 'zod';
import { RustRemoteClient, type NativeRemoteBinding } from './remote-adapter';

const identity = '11111111-1111-4111-8111-111111111111';
function binding(): NativeRemoteBinding {
  return {
    createClient: vi.fn(async () => '1'),
    connect: vi.fn(async () => '2'),
    reconnect: vi.fn(async () => '3'),
    poll: vi.fn(async () => '4'),
    operation: vi.fn(async () => '5'),
    cancel: vi.fn(async () => undefined),
    bindTerminal: vi.fn(async () => undefined),
    unbindTerminal: vi.fn(async () => undefined),
    readEvent: vi.fn(async () => null),
    closeClient: vi.fn(async () => undefined),
  };
}

describe('Rust remote client adapter', () => {
  it('creates an inert remote client without auto-pairing or polling', async () => {
    const native = binding();
    const client = await RustRemoteClient.create(native, 'node_123', identity);
    expect(client.handle).toBe('1');
    expect(native.connect).not.toHaveBeenCalled();
    expect(native.poll).not.toHaveBeenCalled();
    expect(await client.connect()).toBe('2');
    expect(native.connect).toHaveBeenCalledWith('1');
  });

  it('validates destination and identity before creating any native object', async () => {
    const native = binding();
    await expect(RustRemoteClient.create(native, 'http://localhost:123', identity)).rejects.toThrow();
    await expect(RustRemoteClient.create(native, 'node_123', 'host-secret')).rejects.toThrow();
    expect(native.createClient).not.toHaveBeenCalled();
  });

  it('validates operation payload and encodes only the current remote allowlist', async () => {
    const native = binding();
    const client = await RustRemoteClient.create(native, 'node_123', identity);
    expect(await client.operation('getSnapshot', {})).toBe('5');
    expect(native.operation).toHaveBeenCalledWith('1', '{"method":"getSnapshot","payload":{}}');
    await expect(client.operation('renameSession', { sessionId: identity, name: '' })).rejects.toThrow();
    expect(native.operation).toHaveBeenCalledTimes(1);
  });

  it('uses caller-supplied runtime schema rather than asserting parsed event types', async () => {
    const native = binding();
    const client = await RustRemoteClient.create(native, 'node_123', identity);
    const schema = z.object({ operationId: z.string(), status: z.literal(0) }).strict();
    expect(await client.readEvent(schema)).toBeNull();
    vi.mocked(native.readEvent).mockResolvedValueOnce('{"operationId":"5","status":0}');
    expect(await client.readEvent(schema)).toEqual({ operationId: '5', status: 0 });
    vi.mocked(native.readEvent).mockResolvedValueOnce('{"operationId":5,"status":0}');
    await expect(client.readEvent(schema)).rejects.toThrow();
  });

  it('retains failed native close ownership and never retries mutations', async () => {
    const native = binding();
    const client = await RustRemoteClient.create(native, 'node_123', identity);
    vi.mocked(native.operation).mockRejectedValueOnce(new Error('uncertain'));
    await expect(client.operation('getSnapshot', {})).rejects.toThrow('uncertain');
    expect(native.operation).toHaveBeenCalledTimes(1);
    vi.mocked(native.closeClient).mockRejectedValueOnce(new Error('busy'));
    await expect(client.close()).rejects.toThrow('busy');
    await client.close();
    await expect(client.connect()).rejects.toThrow('closed');
    expect(native.closeClient).toHaveBeenCalledTimes(2);
  });
});
