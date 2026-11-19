const { generateKeyPairSync } = require('node:crypto');
const { Server, utils } = require('ssh2');

function echo(stream, banner) {
  stream.write(banner);
  let pending = '';
  stream.on('data', bytes => {
    pending += bytes.toString();
    if (pending.length > 64) return stream.end();
    if (/^echo ok[\r\n]+$/.test(pending)) { stream.write('ok\r\n'); pending = ''; }
    const exit = /^exit(?: (\d+))?[\r\n]+$/.exec(pending);
    if (exit) { stream.exit(Number(exit[1] ?? 0)); stream.end(); }
  });
}

const { privateKey } = generateKeyPairSync('rsa', { modulusLength: 2048 });
const hostKey = process.env.SHELLFOX_TEST_SSH_HOST_KEY || privateKey.export({ type: 'pkcs1', format: 'pem' });
const publicKey = process.env.SHELLFOX_TEST_SSH_PUBLIC_KEY ? utils.parseKey(process.env.SHELLFOX_TEST_SSH_PUBLIC_KEY) : null;
const server = new Server({ hostKeys: [hostKey] }, client => {
  // ssh-keyscan closes before authenticating.
  client.on('error', () => {});
  client.on('authentication', ctx => {
    if (ctx.method === 'password' && ctx.username === 'fixture' && ctx.password === process.env.SHELLFOX_TEST_SSH_PASSWORD) ctx.accept();
    else if (ctx.method === 'publickey' && ctx.username === 'fixture' && publicKey && ctx.key.data.equals(publicKey.getPublicSSH()) &&
      (!ctx.signature || publicKey.verify(ctx.blob, ctx.signature, ctx.hashAlgo))) ctx.accept();
    else ctx.reject(publicKey ? ['publickey', 'password'] : ['password']);
  });
  client.on('ready', () => client.on('session', accept => {
    const session = accept();
    session.on('pty', done => done());
    session.on('window-change', done => done?.());
    if (process.env.SHELLFOX_TEST_SSH_INTERACTIVE === '1') {
      session.on('shell', acceptShell => echo(acceptShell(), 'fixture ready\r\n'));
    }
    session.on('exec', (done, reject, { command }) => {
      const token = /shellfox-auth=([0-9a-f]{64})/.exec(command)?.[1];
      if (!token) return reject();
      echo(done(), `\x1b]777;shellfox-auth=${token}\x07fixture ready\r\n`);
    });
  }));
});
server.listen(Number(process.env.SHELLFOX_TEST_SSH_PORT ?? 0), '127.0.0.1', () => console.log(server.address().port));
