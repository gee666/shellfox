import { expect, it } from 'vitest';
import { envVarsSchemaFor, requestSchemas, pathSchema } from './schemas';
it('trims env names, bounds entries and rejects NUL, equals and platform duplicates', () => {
  const windows = envVarsSchemaFor(true), unix = envVarsSchemaFor(false);
  expect(windows.parse([{ name: '  API_KEY ', value: 'a=b line' }])).toEqual([{ name: 'API_KEY', value: 'a=b line' }]);
  for (const env of [[{ name: '', value: '' }], [{ name: '   ', value: '' }], [{ name: 'A=B', value: '' }], [{ name: 'A\0', value: '' }], [{ name: 'A', value: '\0' }], [{ name: 'x'.repeat(257), value: '' }], [{ name: 'A', value: 'x'.repeat(32768) }], Array.from({ length: 201 }, (_, i) => ({ name: 'N' + i, value: '' })), [{ name: 'Path', value: '' }, { name: ' PATH ', value: '' }]]) expect(windows.safeParse(env).success).toBe(false);
  expect(unix.safeParse([{ name: 'Path', value: '' }, { name: 'PATH', value: '' }]).success).toBe(true);
});
it('rejects nontransportable env names/values consistently on Windows and Unix', () => { for (const windows of [true, false]) { const schema = envVarsSchemaFor(windows); for (const name of ['BAD:NAME', 'BAD/NAME', 'BAD NAME', 'BAD\tNAME']) expect(schema.safeParse([{ name, value: 'ok' }]).success).toBe(false); for (const value of ['a\rb', 'a\nb', 'a\r\nb']) expect(schema.safeParse([{ name: 'GOOD', value }]).success).toBe(false); } });
it('bounds clipboard UTF-8 bytes and never accepts arbitrary folder paths', () => {
  expect(requestSchemas.copyText.safeParse({ text: '\n雪' }).success).toBe(true);
  expect(requestSchemas.copyText.safeParse({ text: '雪'.repeat(11000) }).success).toBe(false);
  expect(requestSchemas.openSessionFolder.safeParse({ sessionId: crypto.randomUUID(), path: 'C:\\arbitrary' }).success).toBe(false);
});
it('permits local WSL interop folders without allowing remote UNC or device paths', () => {
  expect(pathSchema.safeParse('\\\\wsl.localhost\\Debian\\home\\user').success).toBe(true);
  expect(pathSchema.safeParse('\\\\wsl$\\Ubuntu\\home\\user').success).toBe(true);
  for (const value of ['\\\\server\\share', '\\\\?\\C:\\folder', '\\\\wsl.localhost']) expect(pathSchema.safeParse(value).success).toBe(false);
});
