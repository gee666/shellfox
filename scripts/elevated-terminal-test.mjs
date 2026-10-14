// No UAC prompt unless --uac is explicitly supplied. Run from a normal terminal.
import { spawn, execFileSync } from 'node:child_process';
import path from 'node:path';
import { StringDecoder } from 'node:string_decoder';
import { root, run, runNode, env } from './common.mjs';
if (process.platform !== 'win32') throw new Error('Administrator terminal tests require Windows.');
runNode('scripts/terminal-native.mjs', [], 300000);
const helper = path.join(root, 'tmp/terminal-native', 'win-' + process.arch, 'Shellfox.ElevatedTerminal.exe');
const profilerIndex = process.argv.indexOf('--profiler-dll');
if (profilerIndex >= 0 && !process.argv[profilerIndex + 1]) throw new Error('--profiler-dll requires a harmless test DLL path.');
run(helper, profilerIndex < 0 ? ['self-test'] : ['self-test', '--profiler-dll', path.resolve(process.argv[profilerIndex + 1])], 60000);
if (process.argv.includes('--uac')) {
  const powershell = path.join(process.env.SystemRoot || 'C:\\Windows', 'System32/WindowsPowerShell/v1.0/powershell.exe');
  const token = () => execFileSync(powershell, ['-NoLogo', '-NoProfile', '-NonInteractive', '-Command', '[bool]([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)'], { windowsHide: true, timeout: 10000, encoding: 'utf8' }).trim();
  if (token() !== 'False') throw new Error('Run --uac from a non-administrator terminal to test separation.');
  console.log('Approve the Windows UAC prompt for the isolated ConPTY helper.');
  const child = spawn(helper, ['launch'], { env: Object.fromEntries(Object.entries(env).filter(([name]) => !/^(?:DOTNET_|COMPLUS_|CORECLR_|COR_)/i.test(name))), windowsHide: true, stdio: 'pipe', shell: false });
  let buffer = '', output = '', ready = false, confirmed = false;
  const decoder = new StringDecoder('utf8');
  const send = frame => child.stdin.write(JSON.stringify(frame) + '\n');
  const result = new Promise((resolve, reject) => {
    const timer = setTimeout(() => { child.stdin.destroy(); reject(new Error('UAC test timed out.')); }, 130000);
    child.stderr.resume();
    child.on('error', reject);
    child.stdout.on('data', bytes => {
      try {
        buffer += bytes.toString('utf8');
        if (buffer.length > 524288) throw new Error('Oversized helper test output.');
        let newline;
        while ((newline = buffer.indexOf('\n')) >= 0) {
          const frame = JSON.parse(buffer.slice(0, newline)); buffer = buffer.slice(newline + 1);
          if (frame.type === 'ready') {
            if (ready) throw new Error('Duplicate READY.'); ready = true;
            send({ type: 'resize', cols: 100, rows: 40 });
            send({ type: 'write', id: 1, data: "Write-Output ('SHELLFOX_ADMIN_' + [bool]([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)); exit 17\r" });
          }
          if (frame.type === 'data') { output += decoder.write(Buffer.from(frame.data, 'base64')); if (output.length > 262144) throw new Error('Oversized PTY test output.'); }
          if (frame.type === 'error') throw new Error(frame.message);
          if (frame.type === 'exit') {
            confirmed = true; output += decoder.end(); clearTimeout(timer);
            if (!ready || frame.exitCode !== 17 || !output.includes('SHELLFOX_ADMIN_True')) throw new Error('Elevated ConPTY token/output/exit test failed.');
            resolve();
          }
        }
      } catch (error) { clearTimeout(timer); child.stdin.destroy(); reject(error); }
    });
    child.on('close', () => { clearTimeout(timer); if (!confirmed) reject(new Error('Helper exited without confirmed cleanup.')); });
  });
  send({ type: 'launch', cwd: root, env: Object.fromEntries(Object.entries(process.env).filter(([name, value]) => value !== undefined && !name.includes('='))), cols: 80, rows: 24 });
  await result;
  if (token() !== 'False') throw new Error('Host token changed.');
  console.log('Elevated embedded PowerShell, nonadmin host token, resize, input/output and confirmed exit passed.');
}
