import { expect, it } from 'vitest';
import { defaultSettings } from '../main/defaults';
import { requestSchemas, responseSchemas, settingsSchema } from './schemas';

it('defaults legacy settings to an independent empty terminalEnv array', () => {
  const { terminalEnv, ...legacy } = defaultSettings;
  expect(terminalEnv).toEqual([]);
  const first = settingsSchema.parse(legacy), second = settingsSchema.parse(legacy);
  first.terminalEnv.push({ name: 'NEW', value: 'one' });
  expect(second.terminalEnv).toEqual([]);
  expect(requestSchemas.saveSettings.parse(legacy).terminalEnv).toBeUndefined();
  expect(Object.hasOwn(requestSchemas.saveSettings.parse(legacy), 'terminalEnv')).toBe(false);
});

it('canonicalizes global names while preserving literal and empty values', () => {
  const terminalEnv = [{ name: ' TOKEN ', value: 'literal $() ; " 雪 = C:\\work' }, { name: 'EMPTY', value: '' }];
  const result = requestSchemas.saveSettings.parse({ ...defaultSettings, terminalEnv });
  expect(result.terminalEnv).toEqual([{ ...terminalEnv[0], name: 'TOKEN' }, terminalEnv[1]]);
  expect(responseSchemas.saveSettings.parse({ ok: true, value: result })).toEqual({ ok: true, value: result });
});

it('reads Unix case-distinct global variables on every client OS but validates writes on the host', () => {
  const terminalEnv = [{ name: 'MODE', value: 'upper' }, { name: 'mode', value: 'lower' }];
  const settings = { ...defaultSettings, terminalEnv };
  expect(settingsSchema.parse(settings).terminalEnv).toEqual(terminalEnv);
  expect(responseSchemas.saveSettings.safeParse({ ok: true, value: settings }).success).toBe(true);
  expect(requestSchemas.saveSettings.safeParse(settings).success).toBe(process.platform !== 'win32');
});

it('rejects invalid global variables at both the settings and write boundaries', () => {
  const invalid = [
    null, {}, [{ name: '', value: '' }], [{ name: 'A=B', value: '' }],
    [{ name: 'A:B', value: '' }], [{ name: 'A/B', value: '' }],
    [{ name: 'A B', value: '' }], [{ name: 'A', value: '\0' }],
    [{ name: 'A', value: 'a\nb' }], [{ name: 'A', value: 'a\rb' }],
    [{ name: 'A', value: '', extra: true }],
    [{ name: 'A', value: '' }, { name: ' A ', value: 'duplicate' }],
    [{ name: 'x'.repeat(257), value: '' }], [{ name: 'A', value: 'x'.repeat(32768) }],
    Array.from({ length: 201 }, (_, i) => ({ name: 'N' + i, value: '' })),
  ];
  for (const terminalEnv of invalid) {
    expect(settingsSchema.safeParse({ ...defaultSettings, terminalEnv }).success).toBe(false);
    expect(requestSchemas.saveSettings.safeParse({ ...defaultSettings, terminalEnv }).success).toBe(false);
  }
});
