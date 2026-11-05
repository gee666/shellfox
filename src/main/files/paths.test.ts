import { randomUUID } from 'node:crypto';
import { expect, it } from 'vitest';
import { fileTransferRequestSchemas } from '../../shared/file-transfer-schemas';
import { fileTarget, type FileSessionContext } from './paths';

const sessionId = randomUUID(), tabId = randomUUID();
function context(cwd: string, profileId?: string): FileSessionContext {
  return { session: () => ({ cwd }), tab: () => ({ cwd, sessionId, terminal: profileId ? { profileId } : null }) };
}
it('resolves local POSIX/Windows paths with the selected tab rather than the session cwd', () => {
  expect(fileTarget(context('/work/tab'), { sessionId, tabId, path: '../a.ts' }, 'linux').native).toBe('/work/a.ts');
  expect(fileTarget(context('C:\\work\\tab'), { sessionId, tabId, path: '../a.ts' }, 'win32').native).toBe('C:\\work\\a.ts');
});
it('supports host-home/global paths with no session but never accepts a standalone tab or URL', () => {
  const c: FileSessionContext = { session: () => undefined, tab: () => undefined };
  expect(fileTarget(c, { path: '/global/project/file.txt' }, 'linux').native).toBe('/global/project/file.txt');
  expect(fileTarget(c, { path: 'C:\\global\\file.txt' }, 'win32').native).toBe('C:\\global\\file.txt');
  expect(() => fileTarget(c, { tabId, path: '/a' }, 'linux')).toThrow('Invalid file location');
  for (const requested of ['https://example.com/file', 'data:text/plain,secret', 'file:///etc/passwd']) expect(() => fileTarget(c, { path: requested }, 'linux')).toThrow();
});
it('rejects arbitrary UNC/device paths, drive-relative paths and alternate data streams', () => {
  for (const requested of ['\\\\server\\share\\a', '\\\\?\\C:\\a', 'C:relative', 'a.txt:secret', 'folder/a.txt:secret']) expect(() => fileTarget(context('C:\\work'), { sessionId, tabId, path: requested }, 'win32')).toThrow();
});
it('retains the WSL tab context when canonical guest paths are used for later saves and assets', () => {
  const c = context('\\\\wsl.localhost\\Ubuntu\\home\\me', 'wsl:Ubuntu'), location = { sessionId, tabId, path: '/home/me/report.html' };
  const read = fileTarget(c, location, 'win32');
  const canonicalGuest = read.display(read.native);
  const write = fileTransferRequestSchemas.beginFileWrite.parse({ ...location, path: canonicalGuest, size: 1, mode: 'replace', revision: '0'.repeat(64), content: 'text' });
  expect(fileTarget(c, { sessionId: write.sessionId, tabId: write.tabId, path: write.path }, 'win32').native).toBe(read.native);
  const asset = fileTransferRequestSchemas.beginFileRead.parse({ ...location, path: canonicalGuest, assetPath: 'style.css' });
  expect(fileTarget(c, { sessionId: asset.sessionId, tabId: asset.tabId, path: asset.path }, 'win32').native).toBe(read.native);
});
it('maps WSL guest paths through the tab distro without shell interpolation', () => {
  const c = context('\\\\wsl.localhost\\Ubuntu\\home\\me', 'wsl:Ubuntu');
  const relative = fileTarget(c, { sessionId, tabId, path: "folder/a';&.ts" }, 'win32');
  expect(relative.native).toBe("\\\\wsl.localhost\\Ubuntu\\home\\me\\folder\\a';&.ts");
  expect(relative.display(relative.native)).toBe("/home/me/folder/a';&.ts");
  expect(fileTarget(c, { sessionId, tabId, path: '/var/a.txt' }, 'win32').native).toBe('\\\\wsl.localhost\\Ubuntu\\var\\a.txt');
  expect(() => fileTarget(c, { sessionId, tabId, path: '\\\\wsl.localhost\\Debian\\etc\\passwd' }, 'win32')).toThrow('another WSL');
  expect(() => fileTarget(c, { sessionId, tabId, path: 'C:\\work' }, 'win32')).toThrow();
});
