import { randomBytes, randomUUID } from 'node:crypto';
import { spawn } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { createServer } from 'node:http';
import path from 'node:path';
import { expect, test } from 'vitest';
import { RemoteHost } from '../../../src/main/remote/host';
import { embeddedFixture, identity, signedHeaders } from '../../../src/main/remote/test-fixtures';

const rustTest = process.env.SHELLFOX_TEST_RUST_HOST_CLIENT;

// Runs the real Electron host and control broker. Only tsnet dialing and the
// child PTY are replaced; both listeners and all test state are local.
test('the Rust v1 client drives an Electron terminal', async () => {
  if (!rustTest) throw new Error('Set SHELLFOX_TEST_RUST_HOST_CLIENT to the compiled host-client test binary.');
  const directory = await mkdtemp(path.resolve('tmp/electron-v1-'));
  const shell = await embeddedFixture();
  const secret = randomBytes(32).toString('hex');
  const host = new RemoteHost(shell.service, secret, peer => peer === identity.id, () => {}, undefined, undefined, { root: directory });
  const origin = await host.listen();
  const bridge = createServer((request, response) => {
    void (async () => {
      const route = request.url;
      if (request.method !== 'POST' || !route || !['/v1/pair', '/v1/request', '/v1/events', '/v1/disconnect'].includes(route)) {
        response.writeHead(404).end();
        return;
      }
      const chunks: Buffer[] = [];
      for await (const chunk of request) chunks.push(Buffer.from(chunk));
      const body = Buffer.concat(chunks).toString('utf8');
      const result = await fetch(origin + route, { method: 'POST', body, headers: signedHeaders(secret, identity, route, body) });
      response.writeHead(result.status, { 'Content-Type': 'application/json' }).end(await result.text());
    })().catch(() => response.writeHead(500).end());
  });
  try {
    expect(await shell.service.createSession({ cwd: '/work', requestId: randomUUID() })).toMatchObject({ ok: true });
    const [pty] = shell.processes;
    if (!pty) throw new Error('Fixture PTY was not created.');
    pty.output('\x1b[2J\x1b[H\x1b[32mFIXTURE-ANSI\x1b[0m\r\n');
    pty.write.mockImplementation((data: string) => pty.output('FIXTURE-ECHO:' + data));
    await new Promise<void>(resolve => bridge.listen(0, '127.0.0.1', resolve));
    const address = bridge.address();
    if (!address || typeof address === 'string') throw new Error('No fixture bridge');
    const child = spawn(rustTest, ['electron_remote_host_v1_terminal_interop', '--ignored', '--nocapture'], {
      stdio: 'inherit', env: { ...process.env, SHELLFOX_TEST_ELECTRON_V1: `http://127.0.0.1:${address.port}` },
    });
    const timeout = setTimeout(() => child.kill(), 20_000);
    try {
      const code = await new Promise<number | null>((resolve, reject) => { child.once('error', reject); child.once('close', resolve); });
      expect(code).toBe(0);
      expect(pty.resize).toHaveBeenCalledWith(93, 31);
    } finally {
      clearTimeout(timeout);
    }
  } finally {
    bridge.closeAllConnections();
    await new Promise<void>(resolve => bridge.close(() => resolve()));
    await host.dispose();
    await shell.service.dispose();
    await rm(directory, { recursive: true, force: true });
  }
}, 30_000);
