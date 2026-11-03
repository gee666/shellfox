import { expect, it } from 'vitest';
import { defaultSettings } from '../main/defaults';
import { requestSchemas, responseSchemas } from './schemas';
import { remoteForwardRequestSchema, remoteMethodSchema } from './remote-schemas';

it('allows only the narrow global environment operation remotely', () => {
  expect(remoteMethodSchema.safeParse('setTerminalEnv').success).toBe(true);
  expect(remoteMethodSchema.safeParse('saveSettings').success).toBe(false);
  const envelope = { peerId: 'host', method: 'setTerminalEnv', payload: { env: [] } };
  expect(remoteForwardRequestSchema.safeParse(envelope).success).toBe(true);
  for (const payload of [{}, { env: null }, { env: [], pythonPath: '/arbitrary' }, { env: [], processRules: [] }]) {
    expect(requestSchemas.setTerminalEnv.safeParse(payload).success).toBe(false);
    expect(remoteForwardRequestSchema.safeParse({ ...envelope, payload }).success).toBe(false);
  }
});

it('leaves case-sensitive uniqueness to the receiving host and enforces encoded byte limits', () => {
  const env = [{ name: 'MODE', value: 'upper' }, { name: 'mode', value: 'lower' }];
  const envelope = { peerId: 'host', method: 'setTerminalEnv', payload: { env } };
  expect(remoteForwardRequestSchema.safeParse(envelope).success).toBe(true);
  expect(requestSchemas.setTerminalEnv.safeParse({ env }).success).toBe(process.platform !== 'win32');
  const oversized = Array.from({ length: 40 }, (_, i) => ({ name: `N${i}`, value: 'x'.repeat(32767) }));
  expect(remoteForwardRequestSchema.safeParse({ ...envelope, payload: { env: oversized } }).success).toBe(false);
});

it('validates global environment responses without leaking unrecognized fields', () => {
  expect(responseSchemas.setTerminalEnv.parse({ ok: true, value: defaultSettings })).toEqual({ ok: true, value: defaultSettings });
  for (const value of [{ terminalEnv: [] }, { ...defaultSettings, secret: 'private' }, { ...defaultSettings, terminalEnv: [{ name: 'BAD=NAME', value: '' }] }]) {
    expect(responseSchemas.setTerminalEnv.safeParse({ ok: true, value }).success).toBe(false);
  }
});
