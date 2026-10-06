import { test, expect, chromium, _electron, type Browser, type Page, type ElectronApplication } from '@playwright/test';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { createRequire } from 'node:module';
import { createServer } from 'node:net';
import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { shellfoxShims } from '../../src/main/platform/shellfox-cli';
import { launch, scratch, root, testMain, snapshot } from '../fixtures/electron';
const exec = promisify(execFile);
const load = createRequire(path.resolve('package.json'));
const wsl = path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/wsl.exe');
async function run(args: string[]) { return (await exec(wsl, args, { timeout: 30000, maxBuffer: 65536, encoding: 'utf8' })).stdout.trim(); }
async function port() { const server = createServer(); await new Promise<void>(resolve => server.listen(0,'127.0.0.1',resolve)); const value = (server.address() as { port: number }).port; await new Promise<void>(resolve => server.close(() => resolve())); return value; }
async function wait<T>(action: () => Promise<T | null>, timeout=30000): Promise<T> { const end = Date.now()+timeout; while(Date.now()<end) { const result = await action(); if(result) return result; await new Promise(resolve => setTimeout(resolve,200)); } throw new Error('WSL dispatch timed out'); }

async function exitCold(inspectorPort: number) {
  const targets = await (await fetch('http://127.0.0.1:' + inspectorPort + '/json/list')).json() as { webSocketDebuggerUrl: string }[];
  await new Promise<void>((resolve, reject) => {
    const socket = new WebSocket(targets[0].webSocketDebuggerUrl), timer = setTimeout(() => { socket.close(); reject(new Error('Cold test process did not exit')); }, 10000);
    socket.onopen = () => socket.send(JSON.stringify({ id: 1, method: 'Runtime.evaluate', params: { expression: "setImmediate(() => process.exit(0)); true" } }));
    socket.onmessage = event => { const message = JSON.parse(String(event.data)); if (message.id === 1) { clearTimeout(timer); socket.close(); if (message.result?.exceptionDetails) reject(new Error(JSON.stringify(message.result.exceptionDetails))); else resolve(); } };
    socket.onclose = () => { clearTimeout(timer); resolve(); };
    socket.onerror = () => { clearTimeout(timer); reject(new Error('Cold inspector cleanup failed')); };
  });
}

test('transient WSL callers dispatch warm/cold sessions for mounted drives and guest home', async () => {
  test.skip(process.platform !== 'win32', 'Windows WSL interop.');
  test.setTimeout(180000);
  let home: string;
  try { home = await run(['-d','Debian','--exec','/bin/sh','-c','printf "%s" "$HOME"']); }
  catch { test.skip(true, 'Debian WSL is unavailable.'); return; }
  const base = await scratch('wsl-transient'), mounted = path.join(base,"mounted space & 'quote' % !"); await mkdir(mounted);
  const mountedGuest = await run(['-d','Debian','--exec','wslpath','-u',mounted]);
  for (const cold of [false,true]) for (const guestHome of [false,true]) {
    const directory = path.join(base,(cold?'cold':'warm')+'-'+(guestHome?'home':'drive')), data = path.join(directory,'data'), bin = path.join(directory,'bin'); await mkdir(bin,{recursive:true});
    const guestCwd = guestHome ? home : mountedGuest, expectedCwd = await run(['-d','Debian','--exec','wslpath','-w',guestCwd]), debugPort = await port(), inspectorPort = await port();
    let app: ElectronApplication | undefined, browser: Browser | undefined, page: Page | undefined;
    try {
      const packaged = process.env.SHELLFOX_REVIEW_EXE;
      if (!cold) {
        if (packaged) {
          const env = { ...process.env, SHELLFOX_TEST_MODE: '1', SHELLFOX_TEST_ROOT: root }; delete (env as NodeJS.ProcessEnv).ELECTRON_RUN_AS_NODE;
          app = await _electron.launch({ executablePath: packaged, args: ['--test-user-data', data, '--test-backend', 'real'], env, timeout: 30000 });
          page = await app.firstWindow(); await page.getByRole('button', { name: 'New session', exact: true }).waitFor();
        } else { const running = await launch(data,'real'); app=running.app; page=running.page; }
      }
      const shims = shellfoxShims({ executable: packaged ?? load('electron') as string, ...(packaged ? {} : { appPath: testMain }), prefixArgs: ['--test-user-data',data,'--test-backend','real',...(cold?['--remote-debugging-port='+debugPort,'--inspect='+inspectorPort]:[])], launchEnv: { SHELLFOX_TEST_MODE:'1', SHELLFOX_TEST_ROOT:root } });
      await writeFile(path.join(bin,'shellfox'),shims.posix); await writeFile(path.join(bin,'shellfox-dispatch.ps1'),shims.dispatcher);
      const guestShim = await run(['-d','Debian','--exec','wslpath','-u',path.join(bin,'shellfox')]);
      // The calling WSL process exits as soon as the shim returns. No keepalive/sleep.
      const output = await run(['-d','Debian','--cd',guestCwd,'--exec','/bin/sh','-c','exec /bin/sh "$1" start .','shellfox-transient',guestShim]);
      expect(output).toContain('Shellfox: started session');
      if (cold) {
        browser = await wait(async () => chromium.connectOverCDP('http://127.0.0.1:'+debugPort,{timeout:1000}).catch(()=>null));
        page = await wait(async () => browser!.contexts()[0]?.pages().find(p=>p.url().startsWith('file:')) ?? null);
        await page.waitForFunction(()=>!!window.shellfox);
      }
      await expect.poll(async () => (await snapshot(page!)).sessions.some(session=>session.cwd.toLowerCase()===expectedCwd.toLowerCase()&&session.tabs[0]?.lifecycle==='open'),{timeout:30000}).toBe(true);
      const session = (await snapshot(page!)).sessions.find(s=>s.cwd.toLowerCase()===expectedCwd.toLowerCase())!;
      expect(session.tabs).toHaveLength(1); expect(session.adapterId).toBe('embedded-pty');
      await expect(page!.getByRole('button',{name:'Select session '+session.title,exact:true})).toHaveAttribute('aria-pressed','true');
      await test.info().attach((cold?'cold':'warm')+'-'+(guestHome?'home':'drive'),{body:JSON.stringify({output,cwd:expectedCwd,id:session.id}),contentType:'application/json'});
      const failed = await exec(wsl,['-d','Debian','--cd',guestCwd,'--exec','/bin/sh','-c','exec /bin/sh "$1" start "$2"','shellfox-transient',guestShim,'does-not-exist-shellfox'],{timeout:15000,encoding:'utf8'}).then(()=>null,error=>error);
      expect(failed?.code).not.toBe(0); expect(failed?.stdout??'').not.toContain('Shellfox: started session');
    } finally {
      if(page&&!page.isClosed()) { await page.evaluate(async()=>{const r=await window.shellfox.getSnapshot();if(r.ok)for(const s of r.value.sessions)for(const t of s.tabs)if(t.terminalKind==='embedded'&&t.lifecycle!=='closed')await window.shellfox.closeTab!({tabId:t.id,generation:t.generation!});}).catch(()=>{}); }
      if(app) await app.close();
      else if(page) { await exitCold(inspectorPort); await browser?.close().catch(()=>{}); }
      const key='Software\\Shellfox\\Tests\\'+createHash('sha256').update(data).digest('hex');
      await exec('powershell.exe',['-NoProfile','-EncodedCommand',Buffer.from(`[Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree('${key}', $false)`,'utf16le').toString('base64')],{timeout:15000}).catch(()=>{});
    }
  }
});
