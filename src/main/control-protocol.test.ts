import { describe, expect, it } from 'vitest';
import { randomUUID } from 'node:crypto';
import { ControlFrame, controlEndpointSchema, controlRequestSchema, controlReplySchema } from './control-protocol';

const marker = randomUUID();
describe('control validation and framing', () => {
  it.each([
    null, [], {}, { action: 'unknown' }, { action: 'list', extra: true },
    { action: 'inspect' }, { action: 'inspect', id: '' }, { action: 'inspect', id: 'title' },
    { action: 'inspect', marker: '123' }, { action: 'inspect', id: marker, current: true },
    { action: 'tab-open' }, { action: 'tab-close', id: marker }, { action: 'tab-open', marker, id: marker },
    { action: 'keep-alive', confirmed: true }, { action: 'keep-alive', id: marker }, { action: 'stats', current: true },
    { action: 'delete', id: marker, confirmed: 'true' }, { action: 'archive', marker, confirmed: 1 },
  ])('rejects malformed or unsafe requests %j', request => {
    expect(controlRequestSchema.safeParse(request).success).toBe(false);
  });
  it('accepts the contract without adding context variables or extra fields', () => {
    for (const action of ['list', 'stats', 'processes', 'keep-alive']) expect(controlRequestSchema.safeParse({ action, marker }).success).toBe(true);
    for (const action of ['inspect', 'delete', 'archive']) expect(controlRequestSchema.safeParse({ action, id: marker.slice(0, 8), confirmed: action === 'inspect' ? undefined : true }).success).toBe(true);
    expect(controlRequestSchema.safeParse({ action: 'tab-close', marker, current: true }).success).toBe(true);
    expect(controlRequestSchema.safeParse({ action: 'list', marker, current: true }).success).toBe(true);
    expect(controlRequestSchema.safeParse({ action: 'stats', id: marker }).success).toBe(true);
  });
  it('limits endpoint destinations to literal IPv4 loopback and validates secrets', () => {
    const good = { version: 1, host: '127.0.0.1', port: 12345, secret: 'a'.repeat(64) };
    expect(controlEndpointSchema.safeParse(good).success).toBe(true);
    for (const change of [{ host: 'localhost' }, { host: '192.168.1.1' }, { port: 0 }, { port: 65536 }, { port: 1.1 }, { version: 2 }, { secret: 'a' }, { path: '/socket' }]) expect(controlEndpointSchema.safeParse({ ...good, ...change }).success).toBe(false);
  });
  it('validates replies rather than trusting an endpoint response', () => {
    expect(controlReplySchema.safeParse({ ok: true, response: { message: 'ok' } }).success).toBe(true);
    for (const reply of [{ ok: true, response: { unknown: true } }, { ok: true, response: { session: {} } }, { ok: false, error: '' }, { ok: true, response: {}, error: 'no' }]) expect(controlReplySchema.safeParse(reply).success).toBe(false);
  });
  it('handles split UTF-8, rejects invalid UTF-8, extra frames and trailing data', () => {
    const frame = new ControlFrame(30), bytes = Buffer.from('{"a":"é"}\n');
    expect(frame.push(bytes.subarray(0, 7))).toBeUndefined();
    expect(frame.push(bytes.subarray(7))).toBe('{"a":"é"}');
    expect(() => frame.push(Buffer.from('\n'))).toThrow('extra');
    expect(() => new ControlFrame(30).push(Buffer.from('{}\n{}\n'))).toThrow('trailing');
    expect(() => new ControlFrame(30).push(Buffer.from([0xff, 10]))).toThrow();
  });
  it('counts bytes across fragments and bounds unterminated data', () => {
    const frame = new ControlFrame(5);
    expect(frame.push(Buffer.from('123'))).toBeUndefined();
    expect(() => frame.push(Buffer.from('456'))).toThrow('too large');
    expect(() => new ControlFrame(5).push(Buffer.from('ééé\n'))).toThrow('too large');
  });
});
