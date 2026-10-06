import { runNode } from './common.mjs';
const mode = process.argv[2];
if (!['e2e', 'windows', 'packaged'].includes(mode)) throw new Error('Unknown verification command');
if (mode === 'windows' && (process.platform !== 'win32' || process.arch !== 'x64' || process.env.SHELLFOX_WINDOWS_GUI !== '1')) {
  throw new Error('Windows GUI verification requires Windows x64 and SHELLFOX_WINDOWS_GUI=1. It creates only test-owned terminals, never Explorer/installer registrations.');
}
if (mode === 'e2e' || mode === 'windows') runNode('scripts/build.mjs', ['--test'], 600000);
runNode('node_modules/@playwright/test/cli.js', ['test', '--project=' + mode], mode === 'e2e' ? 300000 : 600000, { SHELLFOX_VERIFY_PROJECT: mode });
