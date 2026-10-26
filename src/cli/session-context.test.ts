import { expect, it, vi } from 'vitest';
import { resolveSessionId } from './session-context';
import { parseSessionAction } from './session';
import type { ControlCliIo } from '../main/control-cli';
import { parseControlArgs } from '../main/control-cli';
const id = '11111111-1111-4111-8111-111111111111', marker = '22222222-2222-4222-8222-222222222222';
const options = { json: false, yes: false };
it('an explicit ID wins without consulting an ambient marker, including a remote target', async () => {
  const request = vi.fn<ControlCliIo['request']>();
  expect(await resolveSessionId(id, '/data', { ...options, host: 'remote' }, marker, request)).toBe(id);
  expect(request).not.toHaveBeenCalled();
  expect(parseSessionAction('rename', [id, 'title', '--current'])).toMatchObject({ sessionId: id });
});
it('never sends local terminal context to a remote host', async () => {
  const request = vi.fn<ControlCliIo['request']>();
  await expect(resolveSessionId(undefined, '/data', { ...options, host: 'remote' }, marker, request)).rejects.toThrow('remote session ID');
  expect(request).not.toHaveBeenCalled();
});
it('fails closed without a valid local marker or session identity', async () => {
  const request = vi.fn<ControlCliIo['request']>(async () => ({}));
  await expect(resolveSessionId(undefined, '/data', options, '', request)).rejects.toThrow('No current session');
  expect(request).not.toHaveBeenCalled();
  await expect(resolveSessionId(undefined, '/data', options, marker, request)).rejects.toThrow('live owned session');
  expect(request).toHaveBeenCalledWith('/data', { action: 'inspect', marker, current: true });
});
it('models one explicit target and only the aliases requested by the user', () => {
  expect(parseSessionAction('rename', ['title'])).toMatchObject({ sessionId: undefined, payload: { title: 'title' } });
  expect(parseSessionAction('rename', ['title', '--id', id])).toMatchObject({ sessionId: id });
  for (const operation of ['delete', 'rm', '-rm']) expect(parseSessionAction(operation, [])).toMatchObject({ operation: 'delete' });
  expect(() => parseSessionAction('rename', [id, 'title', '--id', id])).toThrow('one explicit');
  expect(() => parseSessionAction('rename', [id])).toThrow('Missing the new title');
  expect(parseSessionAction('rename', [id, '--id', marker])).toMatchObject({ sessionId: marker, payload: { title: id } });
  expect(() => parseControlArgs(['session', 'status'])).toThrow();
  expect(parseControlArgs(['session', 'stats', '--all'])).toMatchObject({ filters: { all: true } });
});
