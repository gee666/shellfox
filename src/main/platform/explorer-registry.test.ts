import { expect, it } from 'vitest';
import { execFile } from 'node:child_process';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { WindowsExplorer, EXPLORER_OWNER } from './explorer';

function powershell(script: string): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile(path.win32.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/WindowsPowerShell/v1.0/powershell.exe'),
      ['-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(script, 'utf16le').toString('base64')],
      { shell: false, windowsHide: true, timeout: 10000, maxBuffer: 65536, encoding: 'utf8' },
      (error, stdout) => error ? reject(error) : resolve(stdout.replace(/^\uFEFF/, '').trim()));
  });
}
// Only isolated HKCU test keys. Never reads/writes the production Explorer verbs.
it.skipIf(process.platform !== 'win32')('installs, upgrades, removes owned verbs and preserves foreign registry keys', async () => {
  const root = 'Software\\ShellfoxTests\\' + randomUUID(), keys = [root + '\\Folder', root + '\\Background'];
  const legacyKeys = [root + '\\LegacyFolder', root + '\\LegacyBackground'];
  const run = async (script: string) => {
    const encoded = script.match(/FromBase64String\('([^']+)'\)/)![1];
    const payload = JSON.parse(Buffer.from(encoded, 'base64').toString('utf8')); payload.keys = keys; payload.legacyKeys = legacyKeys;
    const isolated = script.replace(encoded, Buffer.from(JSON.stringify(payload)).toString('base64'));
    return JSON.parse(await powershell(isolated));
  };
  const explorer = new WindowsExplorer({ executable: process.execPath, platform: 'win32', run });
  try {
    // Simulate one app-owned legacy verb and one foreign verb with the old key name.
    await powershell(`$k=[Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('${legacyKeys[0]}');$k.SetValue('PiManagerOwner','PiManager/native-v1');$k.Dispose();$k=[Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('${legacyKeys[1]}');$k.SetValue('PiManagerOwner','foreign');$k.Dispose()`);
    expect(await explorer.get()).toMatchObject({ ok: true, value: { supported: true, installed: false } });
    expect(await powershell(`$k=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('${legacyKeys[0]}');[bool]($null -eq $k)`)).toBe('True');
    expect(await powershell(`$k=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('${legacyKeys[1]}');try{$k.GetValue('PiManagerOwner')}finally{$k.Dispose()}`)).toBe('foreign');
    // set() must also clean old owned verbs, not just the startup read.
    await powershell(`$k=[Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('${legacyKeys[0]}');$k.SetValue('PiManagerOwner','PiManager/native-v1');$k.Dispose()`);
    expect(await explorer.set(true)).toMatchObject({ ok: true, value: { installed: true, folderItemInstalled: true, backgroundInstalled: true } });
    expect(await powershell(`$k=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('${legacyKeys[0]}');[bool]($null -eq $k)`)).toBe('True');
    const upgraded = new WindowsExplorer({ executable: process.execPath, appPath: process.cwd(), platform: 'win32', run });
    expect(await upgraded.get()).toMatchObject({ ok: true, value: { installed: false } });
    expect(await upgraded.set(true)).toMatchObject({ ok: true, value: { installed: true } });
    const label = await powershell(`$k=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('${keys[0]}'); try { $k.GetValue('') } finally { $k.Dispose() }`);
    expect(label).toBe('Open in Shellfox');
    // Background menus have no selected item: Single suppresses the verb, even in the classic menu.
    await powershell(`$k=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('${keys[1]}',$true); try { $k.SetValue('MultiSelectModel','Single') } finally { $k.Dispose() }`);
    expect(await upgraded.get()).toMatchObject({ ok: true, value: { installed: false, folderItemInstalled: true, backgroundInstalled: false } });
    expect(await upgraded.set(true)).toMatchObject({ ok: true, value: { installed: true, backgroundInstalled: true } });
    const backgroundModel = await powershell(`$k=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('${keys[1]}'); try { [string]$k.GetValue('MultiSelectModel') } finally { $k.Dispose() }`);
    expect(backgroundModel).toBe('');
    await powershell(`$k=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('${keys[1]}',$true); try { $k.SetValue('ShellfoxOwner','other-application') } finally { $k.Dispose() }`);
    expect(await explorer.set(true)).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
    // Preflight refused before touching the first key's upgraded command.
    expect(await upgraded.get()).toMatchObject({ ok: true, value: { folderItemInstalled: true, backgroundInstalled: false } });
    expect(await upgraded.set(false)).toMatchObject({ ok: true, value: { installed: false, folderItemInstalled: false, backgroundInstalled: false } });
    const owner = await powershell(`$k=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('${keys[1]}'); try { $k.GetValue('ShellfoxOwner') } finally { $k.Dispose() }`);
    expect(owner).toBe('other-application'); expect(owner).not.toBe(EXPLORER_OWNER);
  } finally {
    await powershell(`[Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree('${root}', $false)`);
  }
}, 30000);
