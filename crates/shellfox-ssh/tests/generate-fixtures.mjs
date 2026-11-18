// Runtime-owned fixtures only. Uses the existing independent Node/ssh2 test encoder.
import { createRequire } from 'node:module';
import { randomBytes, pbkdf2Sync, createCipheriv, generateKeyPairSync } from 'node:crypto';
import { transformSync } from 'esbuild';
import { mkdir, open, realpath, readFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
const source = path.resolve(import.meta.dirname, '../../../tests/fixtures/ssh-test-keys.ts');
const compiled = transformSync(await readFile(source, 'utf8'), { loader: 'ts', format: 'cjs' }).code;
const fixtureModule = { exports: {} };
// Execute only this repository-owned test encoder, never a supplied key or config.
new Function('require', 'module', 'exports', compiled)(createRequire(source), fixtureModule, fixtureModule.exports);
const { generatedTestKey, generatedPpk, ed25519SeedFixture, shortEcdsa384Fixture } = fixtureModule.exports;
const directory = await realpath(process.argv[2]);
const root = await realpath(path.resolve(import.meta.dirname, '../../../tmp'));
if (!directory.startsWith(root + path.sep)) throw new Error('fixtures require project tmp');
const helper = process.env.SHELLFOX_SSH_TEST_PERMISSIONS_HELPER;
const protect = file => {
  if (process.platform === 'win32') {
    if (!helper || !path.isAbsolute(helper)) throw new Error('explicit permissions helper required');
    execFileSync(helper, ['--private-permissions', file, 'file', 'protect'], { timeout: 8000, stdio: 'pipe' });
  }
};
// Test-only ASN.1 encoder around Node's PBKDF2/AES primitives, independent of Rust.
const der = (tag, value) => {
  const length = value.length < 128 ? Buffer.from([value.length]) : (() => {
    let hex = value.length.toString(16); if (hex.length % 2) hex = '0' + hex;
    const size = Buffer.from(hex, 'hex'); return Buffer.concat([Buffer.from([0x80 + size.length]), size]);
  })();
  return Buffer.concat([Buffer.from([tag]), length, value]);
};
const seq = (...parts) => der(0x30, Buffer.concat(parts));
const oid = hex => der(6, Buffer.from(hex, 'hex'));
const legacyPkcs8 = key => {
  const salt = randomBytes(16), iv = randomBytes(16), password = 'fixture-legacy-password';
  const aesKey = pbkdf2Sync(password, salt, 2048, 32, 'sha1');
  const cipher = createCipheriv('aes-256-cbc', aesKey, iv);
  const encrypted = Buffer.concat([cipher.update(key.export({ type:'pkcs8', format:'der' })), cipher.final()]);
  aesKey.fill(0);
  const kdf = seq(oid('2a864886f70d01050c'), seq(der(4,salt), der(2,Buffer.from([8,0]))));
  const encryption = seq(oid('60864801650304012a'), der(4,iv));
  const container = seq(seq(oid('2a864886f70d01050d'), seq(kdf,encryption)), der(4,encrypted));
  return `-----BEGIN ENCRYPTED PRIVATE KEY-----\n${container.toString('base64').match(/.{1,64}/g).join('\n')}\n-----END ENCRYPTED PRIVATE KEY-----\n`;
};
const cases = [], legacy = [];
const append = (name, data, fixture, passphrase) => cases.push({ name, data: Buffer.from(data).toString('base64'), public: fixture.publicBlob.toString('base64'), ...(passphrase !== undefined ? { passphrase } : {}) });
for (const kind of ['rsa','ecdsa256','ecdsa384','ecdsa521','ed25519']) {
  const fixture = generatedTestKey(kind);
  append(kind + '-openssh', fixture.openssh, fixture);
  append(kind + '-legacy-pbkdf2-sha1', legacyPkcs8(fixture.keyObject), fixture, 'fixture-legacy-password');
  if (kind === 'rsa' || kind.startsWith('ecdsa')) {
    append(kind + '-traditional-pem', fixture.keyObject.export({type: kind === 'rsa' ? 'pkcs1' : 'sec1',format:'pem'}), fixture);
    legacy.push({name:kind+'-encrypted-traditional',data:Buffer.from(fixture.keyObject.export({type:kind==='rsa'?'pkcs1':'sec1',format:'pem',cipher:'aes-256-cbc',passphrase:'legacy-fixture-only'})).toString('base64'),password:'legacy-fixture-only'});
  }
  for (const version of [2,3]) for (const passphrase of [undefined, 'fixture-passphrase', '']) {
    // The retained independent hash-wasm fixture encoder rejects empty Argon2 passwords.
    if (version === 3 && passphrase === '') continue;
    const options = { ...(passphrase !== undefined ? { passphrase } : {}), newline: '\r\n', comment: 'generated clé / ключ', extraPaddingBlock: true };
    append(`${kind}-ppk${version}-${passphrase === undefined ? 'plain' : passphrase ? 'encrypted' : 'empty-password'}`, await generatedPpk(fixture, version, options), fixture, passphrase);
  }
  for (const passphrase of [undefined, 'fixture-passphrase', '']) {
    append(`${kind}-pkcs8-${passphrase === undefined ? 'plain' : passphrase ? 'encrypted' : 'empty-password'}`,
      fixture.keyObject.export({ type: 'pkcs8', format: 'pem', ...(passphrase !== undefined ? { cipher:'aes-256-cbc', passphrase } : {}) }), fixture, passphrase);
  }
}
for (const first of [0, 128, 255]) for (const version of [2,3]) {
  const key = ed25519SeedFixture(first); append(`seed-${first}-v${version}`, await generatedPpk(key, version), key);
}
for (const version of [2,3]) {
  const key = shortEcdsa384Fixture(); append(`short-p384-v${version}`, await generatedPpk(key, version), key);
}
const dsa = generateKeyPairSync('dsa', {modulusLength:1024,divisorLength:160}).privateKey;
for (const password of [undefined,'legacy-fixture-only']) {
  legacy.push({name:'dsa-pkcs8',data:Buffer.from(dsa.export({type:'pkcs8',format:'pem',...(password?{cipher:'aes-256-cbc',passphrase:password}:{})})).toString('base64'),...(password?{password}:{})});
}
await mkdir(directory, { recursive: true, mode: 0o700 });
const file = path.join(directory, 'fixtures.json');
const handle = await open(file, 'wx', 0o600);
try { protect(file); await handle.writeFile(JSON.stringify({ cases, legacy })); await handle.sync(); }
finally { await handle.close(); }
console.log(`Generated ${cases.length} synthetic private-key fixtures.`);
