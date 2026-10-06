import { expect, it, vi } from 'vitest';
import { EXPLORER_KEYS, EXPLORER_OWNER, LEGACY_EXPLORER_KEYS, LEGACY_EXPLORER_OWNER, WindowsExplorer, explorerCommand } from './explorer';
import { failure, success } from '../../shared/contracts';
it('keeps folder/root arguments quoted and root-safe using the existing CLI flags', () => {
  expect(explorerCommand('C:\\Program Files\\Shellfox.exe', false)).toBe('"C:\\Program Files\\Shellfox.exe" --new-session --cwd "%1\\."');
  expect(explorerCommand('C:\\app.exe', true)).toBe('"C:\\app.exe" --new-session --cwd "%V\\."');
  expect(explorerCommand('C:\\electron.exe', false, 'P:\\work space')).toBe('"C:\\electron.exe" "P:\\work space" --new-session --cwd "%1\\."');
  for (const exe of ['relative.exe', 'C:\\bad".exe', 'C:\\app.cmd', 'C:\\bad\n.exe']) expect(() => explorerCommand(exe, false)).toThrow();
});
it('uses both existing owned HKCU verbs and validates registry responses', async () => {
  const state = { supported: true, installed: true, folderItemInstalled: true, backgroundInstalled: true, reason: null };
  const run = vi.fn(async (_script: string) => success(state)), explorer = new WindowsExplorer({ executable: 'C:\\app.exe', platform: 'win32', run });
  expect(await explorer.set(true)).toEqual(success(state));
  const script = run.mock.calls[0][0], encoded = script.match(/FromBase64String\('([^']+)'\)/)![1];
  expect(JSON.parse(Buffer.from(encoded, 'base64').toString())).toEqual({ keys: EXPLORER_KEYS, owner: EXPLORER_OWNER, legacyKeys: LEGACY_EXPLORER_KEYS, legacyOwner: LEGACY_EXPLORER_OWNER, commands: [explorerCommand('C:\\app.exe', false), explorerCommand('C:\\app.exe', true)], executable: 'C:\\app.exe', installed: true });
  expect(script).toContain("$key.SetValue('', 'Open in Shellfox')");
  expect(script).toContain("if ($key.GetValue('ShellfoxOwner') -cne $p.owner) { throw 'AUTH_FAILED' }");
  expect(script).toContain('if ($owned) { [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree');
  run.mockResolvedValueOnce(failure('AUTH_FAILED', 'Foreign owner') as never); expect((await explorer.set(false)).ok).toBe(false);
  run.mockResolvedValueOnce({ ok: true, value: { ...state, unknown: true } } as never); expect(await explorer.get()).toMatchObject({ ok: false, error: { code: 'NATIVE_UNAVAILABLE' } });
});
it('never invokes Windows registry operations on another platform', async () => {
  const run = vi.fn(), explorer = new WindowsExplorer({ executable: '/app', platform: 'linux', run });
  expect(await explorer.get()).toMatchObject({ ok: true, value: { supported: false } });
  expect(await explorer.set(true)).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
  expect(run).not.toHaveBeenCalled();
});
