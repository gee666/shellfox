import { test, expect, chromium, _electron, type Browser, type Page, type ElectronApplication } from '@playwright/test';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { createRequire } from 'node:module';
import { createServer } from 'node:net';
import { access, mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { z } from 'zod';
import { shellfoxShims } from '../../src/main/platform/shellfox-cli';
import { connectHostClient, type HostClient } from '../../src/host/client';
import { hostEndpointPath } from '../../src/host/rpc';
import { launch, scratch, root, testMain, snapshot } from '../fixtures/electron';
import { stageCliResources } from '../fixtures/cli-resources';
const exec = promisify(execFile);
const load = createRequire(path.resolve('package.json'));
const wsl = path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/wsl.exe');
async function run(args: string[]) { return (await exec(wsl, args, { timeout: 30000, maxBuffer: 65536, encoding: 'utf8' })).stdout.trim(); }
async function port() { const server = createServer(); await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve)); const address = server.address(); if (!address || typeof address === 'string') throw new Error('Missing fixture port'); await new Promise<void>(resolve => server.close(() => resolve())); return address.port; }
async function wait<T>(description: string, action: () => Promise<T | null>, timeout = 30000): Promise<T> { const end = Date.now() + timeout; while (Date.now() < end) { const result = await action(); if (result) return result; await new Promise(resolve => setTimeout(resolve, 200)); } throw new Error('WSL dispatch timed out waiting for ' + description); }
const exists = (file: string) => access(file).then(() => true, () => false);
async function connectFixtureHost(data: string) {
  const relative = path.relative(path.join(root, 'tmp'), data);
  if (!relative || relative.startsWith('..') || path.isAbsolute(relative)) throw new Error('Refusing host access outside isolated test data');
  const client = await connectHostClient(data);
  try { expect((await client.status()).mode).toBe('desktop'); return client; }
  catch (error) { await client.close(); throw error; }
}
const nativeRootsSchema = z.array(z.object({ tabId: z.uuid(), sessionId: z.uuid(), generation: z.uuid(), root: z.object({ pid: z.number().int().positive(), environment: z.enum(['local', 'wsl']), marker: z.uuid() }) }));
const inspectorMessage = z.object({ id: z.number().optional(), result: z.object({ result: z.object({ value: z.unknown().optional() }).optional(), exceptionDetails: z.unknown().optional() }).optional() });

async function coldEvaluate(inspectorPort: number, data: string, expression: string): Promise<unknown> {
  const targets = z.array(z.object({ webSocketDebuggerUrl: z.string().url() })).parse(await (await fetch('http://127.0.0.1:' + inspectorPort + '/json/list', { signal: AbortSignal.timeout(10000) })).json());
  if (!targets.length) throw new Error('Cold fixture inspector is unavailable');
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(targets[0].webSocketDebuggerUrl);
    const timer = setTimeout(() => { socket.close(); reject(new Error('Cold inspector request timed out')); }, 10000);
    socket.onopen = () => socket.send(JSON.stringify({ id: 1, method: 'Runtime.evaluate', params: {
      expression: `(() => { if (!process.argv.includes(${JSON.stringify(data)})) throw new Error('Refusing inspector access outside isolated fixture'); return (${expression}); })()`, returnByValue: true,
    } }));
    socket.onmessage = event => {
      try {
        const message = inspectorMessage.parse(JSON.parse(String(event.data)));
        if (message.id !== 1) return;
        clearTimeout(timer); socket.close();
        if (message.result?.exceptionDetails) reject(new Error(JSON.stringify(message.result.exceptionDetails)));
        else resolve(message.result?.result?.value);
      } catch (error) { clearTimeout(timer); socket.close(); reject(error); }
    };
    socket.onclose = () => { clearTimeout(timer); reject(new Error('Cold inspector closed before replying')); };
    socket.onerror = () => { clearTimeout(timer); reject(new Error('Cold inspector request failed')); };
  });
}
async function nativeRoots(app: ElectronApplication | undefined, inspectorPort: number, data: string) {
  const raw: unknown = app ? await app.evaluate(({ app }, expected) => {
    if (app.getPath('userData') !== expected) throw new Error('Refusing inspection outside isolated test data');
    const hook: unknown = Reflect.get(globalThis, '__shellfoxTest');
    if (!hook || typeof hook !== 'object' || !('backend' in hook) || !hook.backend || typeof hook.backend !== 'object' || !('live' in hook.backend) || typeof hook.backend.live !== 'function') throw new Error('Real test backend is unavailable');
    return hook.backend.live();
  }, data) : await coldEvaluate(inspectorPort, data, 'globalThis.__shellfoxTest.backend.live()');
  return nativeRootsSchema.parse(raw);
}

test('transient WSL callers dispatch warm/cold sessions for mounted drives and guest home', async () => {
  test.skip(process.platform !== 'win32', 'Windows WSL interop.');
  test.setTimeout(180000);
  await stageCliResources(root);
  let home: string;
  try { home = await run(['-d', 'Debian', '--exec', '/bin/sh', '-c', 'printf "%s" "$HOME"']); }
  catch { test.skip(true, 'Debian WSL is unavailable.'); return; }
  const base = await scratch('wsl-transient'), mounted = path.join(base, "mounted space & 'quote' % !"); await mkdir(mounted);
  const mountedGuest = await run(['-d', 'Debian', '--exec', 'wslpath', '-u', mounted]);
  for (const cold of [false, true]) for (const guestHome of [false, true]) {
    const directory = path.join(base, (cold ? 'cold' : 'warm') + '-' + (guestHome ? 'home' : 'drive')), data = path.join(directory, 'data'), bin = path.join(directory, 'bin'); await mkdir(bin, { recursive: true });
    const guestCwd = guestHome ? home : mountedGuest, expectedCwd = await run(['-d', 'Debian', '--exec', 'wslpath', '-w', guestCwd]), debugPort = await port(), inspectorPort = await port();
    let app: ElectronApplication | undefined, browser: Browser | undefined, page: Page | undefined, coldHost: HostClient | undefined;
    const stderrLog = path.join(directory, 'cold-stderr.log'), startupError = path.join(directory, 'cold-startup-error.log');
    try {
      const packaged = process.env.SHELLFOX_REVIEW_EXE;
      if (!cold) {
        if (packaged) {
          const env = { ...process.env, SHELLFOX_TEST_MODE: '1', SHELLFOX_TEST_ROOT: root }; delete (env as NodeJS.ProcessEnv).ELECTRON_RUN_AS_NODE;
          app = await _electron.launch({ executablePath: packaged, args: ['--test-user-data', data, '--test-backend', 'real'], env, timeout: 30000 });
          page = await app.firstWindow(); await page.getByRole('button', { name: 'New session', exact: true }).waitFor();
        } else { const running = await launch(data, 'real'); app = running.app; page = running.page; }
      }
      let entry = testMain;
      if (cold && !packaged) {
        entry = path.join(bin, 'cold-bootstrap.cjs');
        // Loose dev resources resolve from the project root. The requested WSL
        // folder is already an absolute argument; changing startup cwd cannot change it.
        // Record startup errors and let the app's failure path exit, rather than
        // blocking automation in an invisible native error dialog.
        await writeFile(entry, `const fs=require('node:fs'),util=require('node:util'),electron=require('electron');
if(!process.argv.includes(${JSON.stringify(data)}))throw new Error('Wrong isolated cold fixture');
console.error=(...args)=>fs.appendFileSync(${JSON.stringify(stderrLog)},util.format(...args)+'\\n');
electron.dialog.showErrorBox=(...args)=>fs.appendFileSync(${JSON.stringify(startupError)},JSON.stringify(args)+'\\n');
process.chdir(${JSON.stringify(root)});require(${JSON.stringify(testMain)});\n`);
      }
      const shims = shellfoxShims({ executable: packaged ?? z.string().parse(load('electron')), ...(packaged ? {} : { appPath: entry, cliHelper: path.join(root, 'tmp/build-test/cli/main/cli.cjs'), projectRoot: root }), userData: data,
        prefixArgs: ['--test-user-data', data, '--test-backend', 'real', ...(cold ? ['--remote-debugging-port=' + debugPort, '--inspect=' + inspectorPort] : [])], launchEnv: { SHELLFOX_TEST_MODE: '1', SHELLFOX_TEST_ROOT: root, ...(!packaged ? { SHELLFOX_PROJECT_ROOT: root } : {}) } });
      await writeFile(path.join(bin, 'shellfox'), shims.posix); await writeFile(path.join(bin, 'shellfox-dispatch.ps1'), shims.dispatcher);
      const guestShim = await run(['-d', 'Debian', '--exec', 'wslpath', '-u', path.join(bin, 'shellfox')]);
      // The caller exits immediately. Warm create uses RPC; cold create starts an isolated GUI.
      const output = await run(['-d', 'Debian', '--cd', guestCwd, '--exec', '/bin/sh', '-c', 'exec /bin/sh "$1" session create .', 'shellfox-transient', guestShim]);
      if (cold) {
        expect(output).toContain('Shellfox: started session');
        coldHost = await wait('cold desktop HostRPC readiness', async () => {
          if (await exists(startupError)) throw new Error('Cold fixture startup failed: ' + await readFile(stderrLog, 'utf8').catch(() => readFile(startupError, 'utf8')));
          return await exists(hostEndpointPath(data)) ? connectFixtureHost(data) : null;
        });
        await wait('cold renderer file target', async () => {
          const targets = await fetch('http://127.0.0.1:' + debugPort + '/json/list', { signal: AbortSignal.timeout(1000) })
            .then(response => response.json()).then(raw => z.array(z.object({ url: z.string() })).parse(raw)).catch(() => []);
          return targets.some(target => target.url.startsWith('file:')) ? true : null;
        });
        browser = await chromium.connectOverCDP('http://127.0.0.1:' + debugPort, { timeout: 10000 });
        page = await wait('attached cold renderer', async () => browser!.contexts()[0]?.pages().find(p => p.url().startsWith('file:')) ?? null);
        await page.waitForFunction(() => !!window.shellfox);
        const context = z.object({ pid: z.number().int().positive(), cwd: z.string(), data: z.string(), projectRoot: z.string().optional(), resources: z.string().optional() }).parse(await coldEvaluate(inspectorPort, data,
          '({pid:process.pid,cwd:process.cwd(),data:process.argv[process.argv.indexOf("--test-user-data")+1],projectRoot:process.env.SHELLFOX_PROJECT_ROOT,resources:process.env.SHELLFOX_RESOURCES_PATH})'));
        expect(context.data).toBe(data); expect(context.pid).toBe((await coldHost.status()).pid);
        if (!packaged) { expect(context.cwd).toBe(root); expect(context.projectRoot).toBe(root); expect(context.resources).toBe(path.join(root, 'tmp/build-test')); }
        await test.info().attach('cold-startup-context-' + (guestHome ? 'home' : 'drive'), { body: JSON.stringify(context), contentType: 'application/json' });
      }
      await expect.poll(async () => (await snapshot(page!)).sessions.some(session => session.cwd.toLowerCase() === expectedCwd.toLowerCase() && session.tabs[0]?.lifecycle === 'open'), { timeout: 30000 }).toBe(true);
      const session = (await snapshot(page!)).sessions.find(s => s.cwd.toLowerCase() === expectedCwd.toLowerCase())!;
      expect(session.tabs).toHaveLength(1); expect(session.adapterId).toBe('embedded-pty');
      if (!cold) expect(output).toContain(session.id);
      const owned = await nativeRoots(app, inspectorPort, data);
      expect(owned).toHaveLength(1);
      expect(owned[0]).toMatchObject({ sessionId: session.id, tabId: session.tabs[0].id, generation: session.tabs[0].generation, root: { environment: guestHome ? 'wsl' : 'local' } });
      await expect(page!.getByRole('button', { name: 'Select session ' + session.title, exact: true })).toHaveAttribute('aria-pressed', 'true');
      await test.info().attach((cold ? 'cold' : 'warm') + '-' + (guestHome ? 'home' : 'drive'), { body: JSON.stringify({ output, cwd: expectedCwd, id: session.id, owned }), contentType: 'application/json' });
      const failed = await exec(wsl, ['-d', 'Debian', '--cd', guestCwd, '--exec', '/bin/sh', '-c', 'exec /bin/sh "$1" session create "$2"', 'shellfox-transient', guestShim, 'does-not-exist-shellfox'], { timeout: 15000, encoding: 'utf8' }).then(() => null, error => error);
      expect(failed?.code).not.toBe(0); expect(failed?.stdout ?? '').not.toContain('Shellfox: started session');
      expect((await snapshot(page!)).sessions.map(item => item.id)).toEqual([session.id]);
      expect(await nativeRoots(app, inspectorPort, data)).toEqual(owned);
      // Exercise cold teardown without a renderer handle while real PTYs are
      // still owned. The scoped HostRPC stop must be sufficient on its own.
      if (cold && !guestHome) page = undefined;
    } finally {
      if (app) {
        if (page && !page.isClosed()) { await page.evaluate(async () => { const r = await window.shellfox.getSnapshot(); if (r.ok) for (const s of r.value.sessions) for (const t of s.tabs) if (t.terminalKind === 'embedded' && t.lifecycle !== 'closed') await window.shellfox.closeTab!({ tabId: t.id, generation: t.generation! }); }).catch(() => {}); }
        await app.close();
      } else if (cold) {
        try {
          coldHost ??= await exists(hostEndpointPath(data)) ? await connectFixtureHost(data) : undefined;
          if (coldHost) { expect((await coldHost.status()).mode).toBe('desktop'); await coldHost.stop();
            await expect.poll(() => exists(hostEndpointPath(data))).toBe(false);
          }
        } finally { await coldHost?.close(); await browser?.close().catch(() => {}); }
      }
      const key = 'Software\\Shellfox\\Tests\\' + createHash('sha256').update(data).digest('hex');
      await exec('powershell.exe', ['-NoProfile', '-EncodedCommand', Buffer.from(`[Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree('${key}', $false)`,'utf16le').toString('base64')], { timeout: 15000 }).catch(() => {});
    }
  }
});
