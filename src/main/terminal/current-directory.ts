import { execFile } from 'node:child_process';
import { open, readFile, readlink } from 'node:fs/promises';
import path from 'node:path';
import { pathSchema } from '../../shared/schemas';
import type { TrackingRoot } from './tracking';
import type { UnixIdentity } from './unix-close';

export type DirectoryCommand = (file: string, args: string[]) => Promise<Buffer>;
const execute: DirectoryCommand = (file, args) => new Promise((resolve, reject) => {
  execFile(file, args, { encoding: 'buffer', timeout: 4000, maxBuffer: 65536, windowsHide: true }, (error, stdout) => error ? reject(error) : resolve(stdout));
});
const birth = (row: string) => row.slice(row.lastIndexOf(')') + 2).split(/\s+/)[19];
async function environmentMatches(pid: number, marker: string): Promise<boolean> {
  const file = await open(`/proc/${pid}/environ`, 'r');
  try { const buffer = Buffer.alloc(1024 * 1024); const read = await file.read(buffer, 0, buffer.length, 0); return buffer.subarray(0, read.bytesRead).toString('utf8').split('\0').includes(`SHELLFOX_TERMINAL_MARKER=${marker}`); }
  finally { await file.close(); }
}
export interface LinuxDirectoryReader { text(file: string): Promise<string>; link(file: string): Promise<string>; marked(pid: number, marker: string): Promise<boolean> }
const linuxReader: LinuxDirectoryReader = { text: file => readFile(file, 'utf8'), link: readlink, marked: environmentMatches };
export async function linuxDirectory(root: TrackingRoot, identity: UnixIdentity, reader: LinuxDirectoryReader = linuxReader): Promise<string> {
  const expected = identity.birth.split(':');
  if (expected.length !== 2 || !Number.isSafeInteger(identity.pid) || identity.pid < 1) throw new Error('Owned shell identity is unavailable.');
  const boot = (await reader.text('/proc/sys/kernel/random/boot_id')).trim();
  const before = birth(await reader.text(`/proc/${identity.pid}/stat`));
  if (boot !== expected[0] || before !== expected[1] || !await reader.marked(identity.pid, root.marker)) throw new Error('Owned shell identity changed.');
  const directory = await reader.link(`/proc/${identity.pid}/cwd`);
  if (birth(await reader.text(`/proc/${identity.pid}/stat`)) !== before) throw new Error('Owned shell identity changed.');
  return pathSchema.parse(directory);
}
export const GUEST_DIRECTORY_SCRIPT = `import os,sys,json
pid,expected,marker=int(sys.argv[1]),sys.argv[2],sys.argv[3]
boot,ticks=expected.rsplit(':',1)
assert open('/proc/sys/kernel/random/boot_id').read().strip()==boot
def birth(): return open('/proc/%d/stat'%pid).read().rsplit(')',1)[1].split()[19]
assert birth()==ticks
with open('/proc/%d/environ'%pid,'rb') as f: env=f.read(1048576).split(b'\\0')
assert ('SHELLFOX_TERMINAL_MARKER='+marker).encode() in env
directory=os.readlink('/proc/%d/cwd'%pid)
assert birth()==ticks
print(json.dumps(directory))
`;
export async function guestDirectory(root: TrackingRoot, identity: UnixIdentity, run: DirectoryCommand = execute): Promise<string> {
  if (!root.distro || !/^[0-9a-fA-F-]{32,36}:\d+$/.test(identity.birth)) throw new Error('Authenticated guest shell identity is unavailable.');
  const wsl = path.win32.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'wsl.exe');
  const bytes = await run(wsl, ['--distribution', root.distro, '--exec', 'python3', '-c', GUEST_DIRECTORY_SCRIPT, String(identity.pid), identity.birth, root.marker]);
  const directory: unknown = JSON.parse(bytes.toString('utf8')); return pathSchema.parse(directory);
}
export async function darwinDirectory(identity: UnixIdentity, verify: () => Promise<void>, run: DirectoryCommand = execute): Promise<string> {
  if (!Number.isSafeInteger(identity.pid) || identity.pid < 1) throw new Error('Owned shell identity is unavailable.');
  await verify();
  const output = (await run('/usr/sbin/lsof', ['-a', '-p', String(identity.pid), '-d', 'cwd', '-F0fn'])).toString('utf8');
  await verify();
  const names = output.split('\0').map(field => field.replace(/^\n/, '')).filter(field => field.startsWith('n')).map(field => field.slice(1));
  if (names.length !== 1) throw new Error('The shell current directory could not be determined.');
  return pathSchema.parse(names[0]);
}
