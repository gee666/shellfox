// Drives the real Rust owner with the mobile TS client over the v1 protocol
// (remote::v1_loopback_tests::mobile_v1_client_drives_the_real_rust_owner).
import path from 'node:path';
import { run, root, scratch } from './common.mjs';

const target = process.env.CARGO_TARGET_DIR ?? path.join(root, 'target');
const exe = process.platform === 'win32' ? '.exe' : '';
const client = path.join(scratch, 'remote-loopback', 'client.cjs');
run('cargo', ['build', '-p', 'shellfox-cli', '--bin', 'shellfox-cli'], 1_800_000);
run(process.execPath, [path.join(root, 'node_modules/esbuild/bin/esbuild'), 'mobile/src/client/remote-v1-loopback.ts',
  '--bundle', '--platform=node', '--format=cjs', `--outfile=${client}`, '--log-level=warning']);
run('cargo', ['test', '-p', 'shellfox-service', '--lib', '--', '--ignored', '--exact',
  'remote::v1_loopback_tests::mobile_v1_client_drives_the_real_rust_owner'], 1_800_000, {
  SHELLFOX_TEST_SUPERVISOR: path.join(target, 'debug', `shellfox-cli${exe}`),
  SHELLFOX_TEST_MOBILE_CLIENT: client,
});
